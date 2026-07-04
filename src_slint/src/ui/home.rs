//! home.rs — ホーム画面（統計・睡眠予測・週間チャート・日別詳細）のUI連携
//!
//! 役割 : StatsCard/PredictionCard/WeeklyChart/DayDetail各TSXコンポーネント相当の
//!        表示更新ロジックをまとめる。main.rsのmain()から呼ばれるコールバック
//!        本体はここに集約し、main.rsは配線のみを担当する。
//!
//! 依存 : crate::{MainWindow, DaySummaryVM, SessionVM, CalendarDayVM}, events, prediction, utils
//! 公開 : `AppState`, `SharedState`, `new_shared_state`, `refresh_all`,
//!        `compute_stats`, `apply_tick`, `recompute_prediction`, `update_chart`,
//!        `open_day_detail`, `close_day_detail`, `now_iso`, `set_period`,
//!        `open_calendar`, `close_calendar`, `cal_prev_month`, `cal_next_month`, `cal_select_day`

use crate::core::{events, prediction, utils, Session};
use crate::{CalendarDayVM, CurvePointVM, DaySummaryVM, MainWindow, SessionVM};
use chrono::{Datelike, NaiveDate};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const DAYS_JA: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

// StatsCard.tsx の期間タブ（先週=7日・先月=30日・1年=365日・全期間）相当。
#[derive(Clone, Copy, PartialEq)]
pub enum Period {
    Week,
    Month,
    Year,
    All,
}

impl Period {
    fn from_key(key: &str) -> Self {
        match key {
            "week" => Period::Week,
            "year" => Period::Year,
            "all" => Period::All,
            _ => Period::Month,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Period::Week => "week",
            Period::Month => "month",
            Period::Year => "year",
            Period::All => "all",
        }
    }

    fn days(self) -> Option<i64> {
        match self {
            Period::Week => Some(7),
            Period::Month => Some(30),
            Period::Year => Some(365),
            Period::All => None,
        }
    }
}

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
    // 進行中（まだIDLE_RESUMEが来ていない）睡眠セッションの開始時刻。
    // 暫定睡眠時間の表示用（寝ている最中に一瞬起きて確認する用途）。
    // 絶対時刻で持っているので、tickごとに Local::now() との差分を取り直すだけでよい
    // （awake_hoursのようにInstant経過分を毎回加算する必要がない）。
    open_sleep_start: Option<chrono::NaiveDateTime>,
    week_base: NaiveDate,
    selected_date: Option<String>,
    period: Period,
    cal_view: NaiveDate,
}

pub type SharedState = Arc<Mutex<AppState>>;

pub fn new_shared_state() -> SharedState {
    use chrono::Local;
    let today = Local::now().date_naive();
    Arc::new(Mutex::new(AppState {
        baseline: None,
        open_sleep_start: None,
        week_base: today,
        selected_date: None,
        period: Period::Month,
        cal_view: NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    }))
}

// ── 統計ストリップ（StatsCard.tsx 相当）────────────────────────────────────────

pub fn set_period(window: &MainWindow, state: &SharedState, key: &str) {
    let period = Period::from_key(key);
    state.lock().unwrap().period = period;
    window.set_period_key(period.key().into());
    compute_stats(window, state);
}

pub fn compute_stats(window: &MainWindow, state: &SharedState) {
    let sessions = events::get_sessions().unwrap_or_default();
    let period = state.lock().unwrap().period;

    let now = now_iso();
    let recent: Vec<&Session> = match period.days() {
        Some(days) => {
            use chrono::{Duration, Local};
            let cutoff_ts = (Local::now() - Duration::days(days)).format("%Y-%m-%d %H:%M:%S").to_string();
            sessions.iter().filter(|s| s.start.as_str() >= cutoff_ts.as_str()).collect()
        }
        None => sessions.iter().collect(),
    };

    // 平均睡眠は「セッション数」ではなく「記録日数」で割る。1日に複数セッション
    // （昼寝＋本睡眠など）があると、セッション単位の平均では1日あたりの睡眠時間が
    // 薄まって過小評価されてしまうため、日ごとに睡眠時間を合算してから平均する。
    // PC/Android両方から記録された重複区間は merge_intervals で1本にまとめてから
    // 合算する（重なった分の二重計上を防ぐ）。
    let mut per_day_intervals: std::collections::HashMap<&str, Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)>> = std::collections::HashMap::new();
    for s in &recent {
        let day_key = &s.start[..10.min(s.start.len())];
        if let (Ok(st), Ok(en)) = (
            chrono::NaiveDateTime::parse_from_str(s.start.trim(), "%Y-%m-%d %H:%M:%S"),
            chrono::NaiveDateTime::parse_from_str(s.end.trim(), "%Y-%m-%d %H:%M:%S"),
        ) {
            per_day_intervals.entry(day_key).or_default().push((st, en));
        }
    }
    let per_day: std::collections::HashMap<&str, f64> = per_day_intervals.into_iter()
        .map(|(day, intervals)| {
            let merged = utils::merge_intervals(intervals);
            let hours: f64 = merged.iter().map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0).sum();
            (day, hours)
        })
        .collect();
    let avg = if !per_day.is_empty() {
        Some(per_day.values().sum::<f64>() / per_day.len() as f64)
    } else {
        None
    };
    let last = sessions.last().map(|s| s.duration_hours);

    window.set_days_recorded(format!("{}日", per_day.len()).into());
    window.set_avg_sleep(avg.map(utils::format_duration).unwrap_or_else(|| "—".into()).into());
    window.set_last_sleep(last.map(utils::format_duration).unwrap_or_else(|| "—".into()).into());

    let pred = prediction::predict(&sessions, &now);

    // 進行中（まだ閉じていない）睡眠セッションがあれば、暫定睡眠時間表示のために
    // 開始時刻を保持する。寝ている最中に一瞬起きてタブレットを確認する用途。
    let open_sleep_start = events::current_sleep_start()
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok());

    {
        let mut st = state.lock().unwrap();
        st.baseline = Some(StatsBaseline { awake_hours: pred.awake_hours, computed_at: Instant::now() });
        st.open_sleep_start = open_sleep_start;
    }

    apply_tick(window, state);
}

// 現在時刻・起きてから経過時間・暫定睡眠時間だけを軽量に更新する（10秒ごと）。
pub fn apply_tick(window: &MainWindow, state: &SharedState) {
    window.set_current_time(now_hhmm().into());
    let st = state.lock().unwrap();
    if let Some(b) = st.baseline.as_ref() {
        let elapsed_h = b.computed_at.elapsed().as_secs_f64() / 3600.0;
        let awake = b.awake_hours + elapsed_h;
        window.set_awake_since(utils::format_duration(awake).into());
        window.set_awake_color(awake_color(awake));
    }

    if let Some(start) = st.open_sleep_start {
        use chrono::Local;
        let elapsed_h = (Local::now().naive_local() - start).num_seconds() as f64 / 3600.0;
        window.set_has_open_sleep(true);
        window.set_open_sleep_elapsed(utils::format_duration(elapsed_h.max(0.0)).into());
    } else {
        window.set_has_open_sleep(false);
    }
}

// ── 睡眠予測カード（PredictionCard.tsx 相当）───────────────────────────────────

pub fn recompute_prediction(window: &MainWindow) {
    let h = window.get_bed_hour();
    let m = window.get_bed_minute();
    window.set_bed_time_label(format!("{:02}:{:02}", h, m).into());

    let sessions = events::get_sessions().unwrap_or_default();
    if sessions.is_empty() {
        window.set_has_prediction(false);
        return;
    }
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

// WeeklyChart.tsx の durations/y軸目盛り相当。左軸は0h〜(最大値+1)h、7段階で均等割り。
fn build_y_labels(y_max: f64) -> Vec<slint::SharedString> {
    (0..=6).map(|i| format!("{}h", (y_max * (6 - i) as f64 / 6.0).round() as i64).into()).collect()
}

// WeeklyChart.tsx のy2軸（入眠・起床の時刻軸）相当。7段階で均等割りし、時刻ラベルに変換する。
fn build_y2_labels(y2_min: f64, y2_max: f64) -> Vec<slint::SharedString> {
    (0..=6).map(|i| {
        let v = y2_max - (y2_max - y2_min) * i as f64 / 6.0;
        format!("{:02}:00", (v.floor() as i64).rem_euclid(24)).into()
    }).collect()
}

// Catmull-Romスプラインで2点(p1,p2)間をなめらかに補間する（p0,p3は前後の制御点）。
fn catmull_rom(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let calc = |a: f32, b: f32, c: f32, d: f32| -> f32 {
        0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (calc(p0.0, p1.0, p2.0, p3.0), calc(p0.1, p1.1, p2.1, p3.1))
}

// 週7日ぶんのdaily y座標（Noneはデータ欠損）から、連続する区間ごとに
// Catmull-Romスプラインでサンプル点を密に生成する。折れ線を「曲線・実線」に
// 見せるため、Slint側ではこのサンプル点を小さい点として大量に並べて描画する。
fn build_curve(values: &[Option<f32>]) -> Vec<CurvePointVM> {
    const STEPS: usize = 28;
    let n = values.len().max(1) as f32;
    let points: Vec<Option<(f32, f32)>> = values.iter().enumerate()
        .map(|(i, v)| v.map(|y| ((i as f32 + 0.5) / n, y)))
        .collect();

    let mut result = Vec::new();
    let mut run: Vec<(f32, f32)> = Vec::new();
    let flush = |run: &mut Vec<(f32, f32)>, result: &mut Vec<CurvePointVM>| {
        let n = run.len();
        let start_len = result.len();
        if n == 1 {
            result.push(CurvePointVM { x: run[0].0, y: run[0].1, new_run: true });
        } else if n > 1 {
            for i in 0..n - 1 {
                let p0 = if i == 0 { run[0] } else { run[i - 1] };
                let p1 = run[i];
                let p2 = run[i + 1];
                let p3 = if i + 2 < n { run[i + 2] } else { run[n - 1] };
                for s in 0..STEPS {
                    let t = s as f32 / STEPS as f32;
                    let (x, y) = catmull_rom(p0, p1, p2, p3, t);
                    result.push(CurvePointVM { x, y, new_run: false });
                }
            }
            result.push(CurvePointVM { x: run[n - 1].0, y: run[n - 1].1, new_run: false });
        }
        if let Some(first) = result.get_mut(start_len) {
            first.new_run = true;
        }
        run.clear();
    };
    for p in points {
        match p {
            Some(pt) => run.push(pt),
            None => flush(&mut run, &mut result),
        }
    }
    flush(&mut run, &mut result);
    result
}

pub fn update_chart(window: &MainWindow, state: &SharedState) {
    let (week_base, selected) = {
        let s = state.lock().unwrap();
        (s.week_base, s.selected_date.clone())
    };
    let sessions = events::get_sessions().unwrap_or_default();
    let days = utils::build_week(&sessions, week_base);
    let raw_max = days.iter().map(|d| d.total_hours).fold(0.0_f64, f64::max).max(6.0);
    let y_max = raw_max.ceil() + 1.0;

    let all_y2: Vec<f64> = days.iter().flat_map(|d| [d.bedtime_h, d.waketime_h]).flatten().collect();
    let (y2_min, y2_max) = if all_y2.is_empty() {
        (20.0, 32.0)
    } else {
        let lo = all_y2.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = all_y2.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (lo.floor() - 1.0, hi.ceil() + 1.0)
    };
    let y2_range = (y2_max - y2_min).max(0.001);
    // 折れ線グラフ用: 0.0(上端)〜1.0(下端)の縦位置に正規化する
    let y_frac = |v: f64| (1.0 - ((v - y2_min) / y2_range).clamp(0.0, 1.0)) as f32;

    let vm: Vec<DaySummaryVM> = days.iter().enumerate().map(|(i, d)| {
        let date_str = d.date.format("%Y-%m-%d").to_string();
        let is_active = selected.as_deref() == Some(date_str.as_str());
        DaySummaryVM {
            date: date_str.into(),
            day_label: format!("{} {}/{}", DAYS_JA[i], d.date.month(), d.date.day()).into(),
            duration_label: if d.total_hours > 0.0 { utils::format_duration(d.total_hours).into() } else { "".into() },
            bar_frac: (d.total_hours / y_max) as f32,
            has_data: d.total_hours > 0.0,
            active: is_active,
            bedtime_y: d.bedtime_h.map(y_frac).unwrap_or(0.0),
            bedtime_has: d.bedtime_h.is_some(),
            waketime_y: d.waketime_h.map(y_frac).unwrap_or(0.0),
            waketime_has: d.waketime_h.is_some(),
        }
    }).collect();
    let bedtime_ys: Vec<Option<f32>> = vm.iter().zip(days.iter()).map(|(v, d)| d.bedtime_h.map(|_| v.bedtime_y)).collect();
    let waketime_ys: Vec<Option<f32>> = vm.iter().zip(days.iter()).map(|(v, d)| d.waketime_h.map(|_| v.waketime_y)).collect();
    window.set_bedtime_curve(slint::ModelRc::new(slint::VecModel::from(build_curve(&bedtime_ys))));
    window.set_waketime_curve(slint::ModelRc::new(slint::VecModel::from(build_curve(&waketime_ys))));

    window.set_week(slint::ModelRc::new(slint::VecModel::from(vm)));

    window.set_y_labels(slint::ModelRc::new(slint::VecModel::from(build_y_labels(y_max))));
    window.set_y2_labels(slint::ModelRc::new(slint::VecModel::from(build_y2_labels(y2_min, y2_max))));

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

// ── カレンダーピッカー（CalendarPicker.tsx 相当）───────────────────────────────

fn shift_month(d: NaiveDate, delta: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month() as i32 - 1 + delta;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    NaiveDate::from_ymd_opt(y, m as u32, 1).unwrap()
}

fn build_calendar_days(view: NaiveDate, week_base: NaiveDate) -> Vec<CalendarDayVM> {
    use chrono::Local;
    let today = Local::now().date_naive();
    let first_of_month = NaiveDate::from_ymd_opt(view.year(), view.month(), 1).unwrap();
    let grid_start = utils::week_start(first_of_month);
    let ws = utils::week_start(week_base);
    let we = ws + chrono::Duration::days(6);

    (0..42).map(|i| {
        let d = grid_start + chrono::Duration::days(i);
        CalendarDayVM {
            day: d.day() as i32,
            date: d.format("%Y-%m-%d").to_string().into(),
            in_month: d.month() == view.month(),
            is_today: d == today,
            in_week: d >= ws && d <= we,
        }
    }).collect()
}

fn refresh_calendar(window: &MainWindow, state: &SharedState) {
    let (view, week_base) = {
        let s = state.lock().unwrap();
        (s.cal_view, s.week_base)
    };
    window.set_cal_month_label(format!("{}年{}月", view.year(), view.month()).into());
    window.set_cal_days(slint::ModelRc::new(slint::VecModel::from(build_calendar_days(view, week_base))));
}

pub fn open_calendar(window: &MainWindow, state: &SharedState) {
    {
        let week_base = state.lock().unwrap().week_base;
        state.lock().unwrap().cal_view = NaiveDate::from_ymd_opt(week_base.year(), week_base.month(), 1).unwrap();
    }
    refresh_calendar(window, state);
    window.set_cal_open(true);
}

pub fn close_calendar(window: &MainWindow) {
    window.set_cal_open(false);
}

pub fn cal_prev_month(window: &MainWindow, state: &SharedState) {
    { let mut s = state.lock().unwrap(); s.cal_view = shift_month(s.cal_view, -1); }
    refresh_calendar(window, state);
}

pub fn cal_next_month(window: &MainWindow, state: &SharedState) {
    { let mut s = state.lock().unwrap(); s.cal_view = shift_month(s.cal_view, 1); }
    refresh_calendar(window, state);
}

pub fn cal_select_day(window: &MainWindow, state: &SharedState, date: &str) {
    if let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        state.lock().unwrap().week_base = d;
    }
    window.set_cal_open(false);
    update_chart(window, state);
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

    // PC/Android両方から記録された重複区間は合計計算では1本にまとめて二重計上を
    // 防ぐ（一覧表示では元のセッションをそのまま出し、重複しているものには
    // ⚠マークを付ける）。
    let parse = |s: &str| chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok();
    let day_intervals: Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)> = day_sessions.iter()
        .filter_map(|s| Some((parse(&s.start)?, parse(&s.end)?)))
        .collect();
    let merged = utils::merge_intervals(day_intervals.clone());
    let total: f64 = merged.iter().map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0).sum();

    let vm: Vec<SessionVM> = day_sessions.iter().enumerate().map(|(i, s)| {
        let overlaps = match (parse(&s.start), parse(&s.end)) {
            (Some(st), Some(en)) => day_intervals.iter().enumerate()
                .any(|(j, (os, oe))| j != i && st < *oe && en > *os),
            _ => false,
        };
        SessionVM {
            start: s.start.clone().into(),
            end: s.end.clone().into(),
            time_range: format!("{} → {}", fmt_ts_short(&s.start), fmt_ts_short(&s.end)).into(),
            duration_label: utils::format_duration(s.duration_hours).into(),
            deleting: false,
            overlaps,
        }
    }).collect();

    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or_else(|_| chrono::Local::now().date_naive());
    let next = d + chrono::Duration::days(1);
    window.set_detail_date_label(date_label_ja(d).into());
    window.set_detail_total_label(utils::format_duration(total).into());
    window.set_detail_sessions(slint::ModelRc::new(slint::VecModel::from(vm)));
    window.set_detail_add_open(false);
    window.set_detail_error("".into());
    window.set_detail_add_start_y(d.year());
    window.set_detail_add_start_mo(d.month() as i32);
    window.set_detail_add_start_d(d.day() as i32);
    window.set_detail_add_start_h(23);
    window.set_detail_add_start_m(0);
    window.set_detail_add_end_y(next.year());
    window.set_detail_add_end_mo(next.month() as i32);
    window.set_detail_add_end_d(next.day() as i32);
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
