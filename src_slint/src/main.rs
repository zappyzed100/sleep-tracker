//! main.rs — sleep_tracker (Rust + Slint) エントリポイント・共有状態
//!
//! 役割 : Slintウィンドウの起動、共有static（THRESHOLD_SECS, HTTP_CLIENT）、
//!        パスユーティリティ（data_dir, config_path）を定義する。
//!        各モジュール（config, events, cloud, prediction, monitor, platform）を
//!        宣言し、起動時の初期化（Drive同期・monitorスレッド起動）と
//!        ホーム画面（統計ストリップ・睡眠予測カード・週間チャート）への
//!        データバインディングを行う。
//!
//! 公開 : `THRESHOLD_SECS`, `data_dir`, `config_path`, `http_client`

mod config;
mod events;
mod cloud;
mod prediction;
#[cfg(windows)]
mod monitor;
mod platform;
mod utils;

pub use events::Session;

use chrono::{Datelike, NaiveDate};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

slint::include_modules!();

const DAYS_JA: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

// Shared threshold: updated instantly by save_config, read by monitor thread.
pub static THRESHOLD_SECS: AtomicU64 = AtomicU64::new(3600);

static HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

// ── Path utilities ────────────────────────────────────────────────────────────
//
// Tauri版の repo_root() と同じ考え方: exe の場所から上に辿って src_tauri/ を
// マーカーに探す。開発中は既存Tauri版と同じデータ（sleep_events.txt等）を
// 共有して実データで検証できるようにするための暫定措置。

fn repo_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        let mut dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        for _ in 0..8 {
            if dir.join("src_tauri").exists() { return dir; }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
        std::env::current_dir().unwrap_or_default()
    })
}

pub fn data_dir() -> PathBuf {
    static DATA: OnceLock<PathBuf> = OnceLock::new();
    DATA.get_or_init(|| {
        let dir = repo_root().join("src_tauri").join("data");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }).clone()
}

pub fn config_path() -> PathBuf {
    repo_root().join("config.json")
}

pub fn http_client() -> Result<&'static reqwest::blocking::Client, String> {
    if HTTP_CLIENT.get().is_none() {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let _ = HTTP_CLIENT.set(client);
    }
    Ok(HTTP_CLIENT.get().unwrap())
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

fn now_iso() -> String {
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

struct AppState {
    baseline: Option<StatsBaseline>,
    week_base: NaiveDate,
    selected_date: Option<String>,
}

type SharedState = Arc<Mutex<AppState>>;

fn new_shared_state() -> SharedState {
    use chrono::Local;
    Arc::new(Mutex::new(AppState {
        baseline: None,
        week_base: Local::now().date_naive(),
        selected_date: None,
    }))
}

// ── 統計ストリップ（StatsCard.tsx 相当）────────────────────────────────────────
// StatsCard.tsx の期間タブ「先月」(30日) 相当をデフォルトとして使用。

fn compute_stats(window: &MainWindow, state: &SharedState) {
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
fn apply_tick(window: &MainWindow, state: &SharedState) {
    window.set_current_time(now_hhmm().into());
    if let Some(b) = state.lock().unwrap().baseline.as_ref() {
        let elapsed_h = b.computed_at.elapsed().as_secs_f64() / 3600.0;
        let awake = b.awake_hours + elapsed_h;
        window.set_awake_since(utils::format_duration(awake).into());
        window.set_awake_color(awake_color(awake));
    }
}

// ── 睡眠予測カード（PredictionCard.tsx 相当）───────────────────────────────────

fn recompute_prediction(window: &MainWindow) {
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

fn update_chart(window: &MainWindow, state: &SharedState) {
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

fn refresh_all(window: &MainWindow, state: &SharedState) {
    compute_stats(window, state);
    recompute_prediction(window);
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

fn open_day_detail(window: &MainWindow, state: &SharedState, date: &str) {
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

fn close_day_detail(window: &MainWindow, state: &SharedState) {
    state.lock().unwrap().selected_date = None;
    window.set_show_detail(false);
    update_chart(window, state);
}

fn main() {
    // 起動時初期化: config.jsonからTHRESHOLD_SECSを読み込み
    let cfg = config::load_config_inner();
    if let Some(m) = cfg.idle_threshold_minutes {
        THRESHOLD_SECS.store(m as u64 * 60, Ordering::Relaxed);
    }

    let window = MainWindow::new().expect("ウィンドウの作成に失敗しました");
    window.set_greeting("Rust + Slint 起動成功".into());

    // 睡眠予測カードの初期入眠時刻 = 現在時刻（PredictionCard.tsx の currentHHMM 相当）
    {
        use chrono::Local;
        let now = Local::now();
        window.set_bed_hour(now.format("%H").to_string().parse().unwrap_or(22));
        window.set_bed_minute(now.format("%M").to_string().parse().unwrap_or(0));
    }

    let state = new_shared_state();
    refresh_all(&window, &state);

    // 入眠時刻の手動変更 → 予測を再計算
    {
        let weak = window.as_weak();
        window.on_bedtime_edited(move |_h, _m| {
            if let Some(w) = weak.upgrade() {
                recompute_prediction(&w);
            }
        });
    }

    // 「今すぐ」: 入眠時刻を現在時刻にセットして再計算
    {
        let weak = window.as_weak();
        window.on_set_bedtime_now(move || {
            if let Some(w) = weak.upgrade() {
                use chrono::Local;
                let now = Local::now();
                w.set_bed_hour(now.format("%H").to_string().parse().unwrap_or(22));
                w.set_bed_minute(now.format("%M").to_string().parse().unwrap_or(0));
                recompute_prediction(&w);
            }
        });
    }

    // 「最適睡眠」: find_optimalの結果を入眠時刻にセットして再計算
    {
        let weak = window.as_weak();
        window.on_set_bedtime_optimal(move || {
            if let Some(w) = weak.upgrade() {
                let sessions = events::get_sessions().unwrap_or_default();
                if sessions.is_empty() { return; }
                let cfg = config::load_config_inner();
                let target = cfg.target_wake_time;
                if let Some(opt) = prediction::find_optimal(&sessions, &now_iso(), target.as_deref()) {
                    let parts: Vec<&str> = opt.best_bed_time.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        w.set_bed_hour(parts[0].parse().unwrap_or(22));
                        w.set_bed_minute(parts[1].parse().unwrap_or(0));
                        recompute_prediction(&w);
                    }
                }
            }
        });
    }

    // 週ナビゲーション: 前週/次週/今週
    {
        let weak = window.as_weak();
        let state_nav = state.clone();
        window.on_prev_week(move || {
            if let Some(w) = weak.upgrade() {
                state_nav.lock().unwrap().week_base -= chrono::Duration::days(7);
                update_chart(&w, &state_nav);
            }
        });
    }
    {
        let weak = window.as_weak();
        let state_nav = state.clone();
        window.on_next_week(move || {
            if let Some(w) = weak.upgrade() {
                state_nav.lock().unwrap().week_base += chrono::Duration::days(7);
                update_chart(&w, &state_nav);
            }
        });
    }
    {
        let weak = window.as_weak();
        let state_nav = state.clone();
        window.on_this_week(move || {
            if let Some(w) = weak.upgrade() {
                use chrono::Local;
                state_nav.lock().unwrap().week_base = Local::now().date_naive();
                update_chart(&w, &state_nav);
            }
        });
    }

    // 日クリック: 詳細モーダルを開く
    {
        let weak = window.as_weak();
        let state_click = state.clone();
        window.on_day_clicked(move |date| {
            if let Some(w) = weak.upgrade() {
                open_day_detail(&w, &state_click, &date);
            }
        });
    }

    // モーダルを閉じる
    {
        let weak = window.as_weak();
        let state_close = state.clone();
        window.on_close_detail(move || {
            if let Some(w) = weak.upgrade() {
                close_day_detail(&w, &state_close);
            }
        });
    }

    // 追加フォームの開閉トグル
    {
        let weak = window.as_weak();
        window.on_toggle_add_session(move || {
            if let Some(w) = weak.upgrade() {
                w.set_detail_add_open(!w.get_detail_add_open());
                w.set_detail_error("".into());
            }
        });
    }

    // セッション削除
    {
        let weak = window.as_weak();
        let state_del = state.clone();
        window.on_delete_session(move |start, end| {
            if let Some(w) = weak.upgrade() {
                match events::delete_session(start.to_string(), end.to_string()) {
                    Ok(()) => {
                        refresh_all(&w, &state_del);
                        let date = state_del.lock().unwrap().selected_date.clone();
                        if let Some(d) = date { open_day_detail(&w, &state_del, &d); }
                    }
                    Err(e) => w.set_detail_error(format!("削除失敗: {}", e).into()),
                }
            }
        });
    }

    // 手動追加
    {
        let weak = window.as_weak();
        let state_add = state.clone();
        window.on_add_session(move || {
            if let Some(w) = weak.upgrade() {
                let date = state_add.lock().unwrap().selected_date.clone();
                let Some(date) = date else { return };
                let Ok(d) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else { return };
                let next = d + chrono::Duration::days(1);
                let start = format!("{} {:02}:{:02}:00", d.format("%Y-%m-%d"), w.get_detail_add_start_h(), w.get_detail_add_start_m());
                let end = format!("{} {:02}:{:02}:00", next.format("%Y-%m-%d"), w.get_detail_add_end_h(), w.get_detail_add_end_m());
                if start >= end {
                    w.set_detail_error("起床時刻は入眠時刻より後にしてください".into());
                    return;
                }
                match events::add_session(start, end) {
                    Ok(()) => {
                        refresh_all(&w, &state_add);
                        open_day_detail(&w, &state_add, &date);
                    }
                    Err(e) => w.set_detail_error(format!("追加失敗: {}", e).into()),
                }
            }
        });
    }

    // 10秒ごとに現在時刻・起きてから経過時間を更新
    let timer = slint::Timer::default();
    {
        let weak = window.as_weak();
        let state_timer = state.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(10), move || {
            if let Some(w) = weak.upgrade() {
                apply_tick(&w, &state_timer);
            }
        });
    }

    // 同期ボタン: 別スレッドでsync_gistを実行し、完了後にUIスレッドで再読み込み
    {
        let weak = window.as_weak();
        let state_sync = state.clone();
        window.on_sync_clicked(move || {
            let weak = weak.clone();
            let state_sync = state_sync.clone();
            std::thread::spawn(move || {
                let msg = cloud::sync_gist();
                eprintln!("[app] sync_gist: {:?}", msg);
                let weak = weak.clone();
                let state_sync = state_sync.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        refresh_all(&w, &state_sync);
                    }
                });
            });
        });
    }

    // Drive → ローカルへの起動時同期（別スレッド、完了後にUI再読み込み）
    {
        let weak = window.as_weak();
        let state_startup = state.clone();
        std::thread::spawn(move || {
            cloud::ensure_events_from_drive();
            let _ = cloud::pull_mobile_events_inner();
            let weak = weak.clone();
            let state_startup = state_startup.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    refresh_all(&w, &state_startup);
                }
            });
        });
    }

    // アイドル監視スレッド（Windowsデスクトップのみ）
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        let state_monitor = state.clone();
        monitor::start(data_dir(), move || {
            let weak = weak.clone();
            let state_monitor = state_monitor.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    refresh_all(&w, &state_monitor);
                }
            });
        });
    }

    window.run().expect("ウィンドウの実行に失敗しました");
}
