//! home.rs — ホーム画面（統計・睡眠予測・週間チャート・日別詳細）のUI連携
//!
//! 役割 : StatsCard/PredictionCard/WeeklyChart/DayDetail各TSXコンポーネント相当の
//!        表示更新ロジックをまとめる。main.rsのmain()から呼ばれるコールバック
//!        本体はここに集約し、main.rsは配線のみを担当する。
//!
//! 依存 : crate::{MainWindow, DaySummaryVM, SessionVM}, events, prediction, utils
//! 公開 : `AppState`, `SharedState`, `new_shared_state`, `refresh_all`,
//!        `compute_stats`, `apply_tick`, `recompute_prediction`, `update_chart`,
//!        `open_day_detail`, `close_day_detail`, `now_iso`

use crate::core::{events, prediction, utils, Session};
use crate::{DaySummaryVM, MainWindow, SessionVM};
use chrono::{Datelike, NaiveDate};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const DAYS_JA: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

fn awake_color(h: f64) -> slint::Color {
    if h > 16.0 { slint::Color::from_rgb_u8(0xf3, 0x8b, 0xa8) }      // --red
    else if h > 12.0 { slint::Color::from_rgb_u8(0xf9, 0xe2, 0xaf) } // --yellow
    else { slint::Color::from_rgb_u8(0xa6, 0xe3, 0xa1) }             // --green
}

fn now_hhmm() -> String {
    use chrono::Local;
    Local::now().format("%H:%M").to_string()
}

pub fn now_iso() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn bed_time_to_iso(h: i32, m: i32) -> String {
    use chrono::Local;
    format!("{} {:02}:{:02}:00", Local::now().format("%Y-%m-%d"), h, m)
}

// ── アプリ共有状態 ────────────────────────────────────────────────────────────
//
// バックグラウンドスレッド（Drive同期・監視スレッド）から
// slint::invoke_from_event_loop 経由でUIスレッドに戻すため Arc<Mutex<>> で保持する
// （Rc<RefCell<>> は Send でないため thread::spawn の境界を越えられない）。

struct StatsBaseline {
    awake_hours: f64,
    computed_at: Instant,
}

pub struct AppState {
    baseline: Option<StatsBaseline>,
    week_base: NaiveDate,
    selected_date: Option<String>,
}

pub type SharedState = Arc<Mutex<AppState>>;

pub fn new_shared_state() -> SharedState {
    use chrono::Local;
    Arc::new(Mutex::new(AppState {
        baseline: None,
        week_base: Local::now().date_naive(),
        selected_date: None,
    }))
}

// ── 統計ストリップ（StatsCard.tsx 相当）────────────────────────────────────────
// StatsCard.tsx の期間タブ「先月」(30日) 相当をデフォルトとして使用。

pub fn compute_stats(window: &MainWindow, state: &SharedState) {
    let sessions = events::get_sessions().unwrap_or_default();

    let now = now_iso();
    let thirty_days_ago_ts = {
        use chrono::{Local, Duration};
        (Local::now() - Duration::days(30)).format("%Y-%m-%d %H:%M:%S").to_string()
    };
    let recent: Vec<&Session> = sessions.iter().filter(|s| s.start.as_str() >= thirty_days_ago_ts.as_str()).collect();

    let mut unique_days: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for s in &recent {
        unique_days.insert(&s.start[..10.min(s.start.len())]);
    }
    let avg = if !recent.is_empty() {
        Some(recent.iter().map(|s| s.duration_hours).sum::<f64>() / recent.len() as f64)
    } else {
        None
    };
    let last = sessions.last().map(|s| s.duration_hours);

    window.set_days_recorded(format!("{}日", unique_days.len()).into());
    window.set_avg_sleep(avg.map(utils::format_duration).unwrap_or_else(|| "—".into()).into());
    window.set_last_sleep(last.map(utils::format_duration).unwrap_or_else(|| "—".into()).into());

    let pred = prediction::predict(&sessions, &now);
    state.lock().unwrap().baseline = Some(StatsBaseline { awake_hours: pred.awake_hours, computed_at: Instant::now() });

    apply_tick(window, state);
}

// 現在時刻・起きてから経過時間だけを軽量に更新する（10秒ごと）。
pub fn apply_tick(window: &MainWindow, state: &SharedState) {
    window.set_current_time(now_hhmm().into());
    if let Some(b) = state.lock().unwrap().baseline.as_ref() {
        let elapsed_h = b.computed_at.elapsed().as_secs_f64() / 3600.0;
        let awake = b.awake_hours + elapsed_h;
        window.set_awake_since(utils::format_duration(awake).into());
        window.set_awake_color(awake_color(awake));
    }
}

// ── 睡眠予測カード（PredictionCard.tsx 相当）───────────────────────────────────

pub fn recompute_prediction(window: &MainWindow) {
    let sessions = events::get_sessions().unwrap_or_default();
    if sessions.is_empty() {
        window.set_has_prediction(false);
        return;
    }
    let h = window.get_bed_hour();
    let m = window.get_bed_minute();
    let now_at_bedtime = bed_time_to_iso(h, m);
    let pred = prediction::predict(&sessions, &now_at_bedtime);

    let wake_total_min = h * 60 + m + (pred.duration_hours * 60.0) as i32;
    let wake_h = (wake_total_min / 60).rem_euclid(24);
    let wake_m = wake_total_min.rem_euclid(60);

    window.set_predicted_duration(utils::format_duration(pred.duration_hours).into());
    window.set_predicted_wake_time(format!("{:02}:{:02}", wake_h, wake_m).into());
    window.set_predicted_method(pred.method.into());
    window.set_has_prediction(true);
}

// ── 週間チャート（WeeklyChart.tsx 相当）────────────────────────────────────────

pub fn update_chart(window: &MainWindow, state: &SharedState) {
    let (week_base, selected) = {
        let s = state.lock().unwrap();
        (s.week_base, s.selected_date.clone())
    };
    let sessions = events::get_sessions().unwrap_or_default();
    let days = utils::build_week(&sessions, week_base);
    let max_hours = days.iter().map(|d| d.total_hours).fold(0.0_f64, f64::max).max(6.0);

    let vm: Vec<DaySummaryVM> = days.iter().enumerate().map(|(i, d)| {
        let date_str = d.date.format("%Y-%m-%d").to_string();
        let is_active = selected.as_deref() == Some(date_str.as_str());
        DaySummaryVM {
            date: date_str.into(),
            day_label: format!("{}\n{}/{}", DAYS_JA[i], d.date.month(), d.date.day()).into(),
            duration_label: if d.total_hours > 0.0 { utils::format_duration(d.total_hours).into() } else { "".into() },
            bar_frac: (d.total_hours / max_hours) as f32,
            has_data: d.total_hours > 0.0,
            active: is_active,
        }
    }).collect();
    window.set_week(slint::ModelRc::new(slint::VecModel::from(vm)));

    let fmt = |d: NaiveDate| format!("{}/{:02}/{:02} ({})", d.year(), d.month(), d.day(), DAYS_JA[d.weekday().num_days_from_monday() as usize]);
    let end = week_base + chrono::Duration::days(6 - week_base.weekday().num_days_from_monday() as i64);
    let start = week_base - chrono::Duration::days(week_base.weekday().num_days_from_monday() as i64);
    window.set_week_range_label(format!("{} 〜 {}", fmt(start), fmt(end)).into());
}

pub fn refresh_all(window: &MainWindow, state: &SharedState) {
    compute_stats(window, state);
    recompute_prediction(window);
    update_chart(window, state);
}

pub fn adjust_week(state: &SharedState, days: i64) {
    state.lock().unwrap().week_base += chrono::Duration::days(days);
}

pub fn reset_week_to_today(state: &SharedState) {
    use chrono::Local;
    state.lock().unwrap().week_base = Local::now().date_naive();
}

pub fn selected_date(state: &SharedState) -> Option<String> {
    state.lock().unwrap().selected_date.clone()
}

// ── 日別詳細モーダル（DayDetail.tsx 相当）──────────────────────────────────────

fn date_label_ja(d: NaiveDate) -> String {
    const DOW_SUN_FIRST: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
    format!("{}年{}月{}日（{}）", d.year(), d.month(), d.day(), DOW_SUN_FIRST[d.weekday().num_days_from_sunday() as usize])
}

fn fmt_ts_short(ts: &str) -> String {
    // "2026-07-01 23:00:00" → "7/1 23:00"
    let (date, time) = ts.split_once(' ').unwrap_or((ts, ""));
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    if parts.len() == 3 {
        format!("{}/{} {}", parts[1].trim_start_matches('0'), parts[2].trim_start_matches('0'), time.get(..5).unwrap_or(time))
    } else {
        ts.to_string()
    }
}

pub fn open_day_detail(window: &MainWindow, state: &SharedState, date: &str) {
    state.lock().unwrap().selected_date = Some(date.to_string());

    let sessions = events::get_sessions().unwrap_or_default();
    let day_sessions: Vec<&Session> = sessions.iter().filter(|s| s.start.starts_with(date)).collect();
    let total: f64 = day_sessions.iter().map(|s| s.duration_hours).sum();

    let vm: Vec<SessionVM> = day_sessions.iter().map(|s| SessionVM {
        start: s.start.clone().into(),
        end: s.end.clone().into(),
        time_range: format!("{} → {}", fmt_ts_short(&s.start), fmt_ts_short(&s.end)).into(),
        duration_label: utils::format_duration(s.duration_hours).into(),
        deleting: false,
    }).collect();

    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or_else(|_| chrono::Local::now().date_naive());
    window.set_detail_date_label(date_label_ja(d).into());
    window.set_detail_total_label(utils::format_duration(total).into());
    window.set_detail_sessions(slint::ModelRc::new(slint::VecModel::from(vm)));
    window.set_detail_add_open(false);
    window.set_detail_error("".into());
    window.set_detail_add_start_h(23);
    window.set_detail_add_start_m(0);
    window.set_detail_add_end_h(7);
    window.set_detail_add_end_m(0);
    window.set_show_detail(true);

    update_chart(window, state);
}

pub fn close_day_detail(window: &MainWindow, state: &SharedState) {
    state.lock().unwrap().selected_date = None;
    window.set_show_detail(false);
    update_chart(window, state);
}
