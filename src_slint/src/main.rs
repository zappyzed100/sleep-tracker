//! main.rs — sleep_tracker (Rust + Slint) エントリポイント・共有状態
//!
//! 役割 : Slintウィンドウの起動、共有static（THRESHOLD_SECS, HTTP_CLIENT）、
//!        パスユーティリティ（data_dir, config_path）を定義する。
//!        各モジュール（config, events, cloud, prediction, monitor, platform,
//!        home, settings_ui）を宣言し、起動時の初期化と全コールバックの配線を行う。
//!        表示更新ロジック本体は home.rs / settings_ui.rs に委譲する。
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
mod home;
mod settings_ui;

pub use events::Session;

use chrono::NaiveDate;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

slint::include_modules!();

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

    settings_ui::load_into_window(&window);

    let state = home::new_shared_state();
    home::refresh_all(&window, &state);

    // ── 睡眠予測カード ──
    {
        let weak = window.as_weak();
        window.on_bedtime_edited(move |_h, _m| {
            if let Some(w) = weak.upgrade() { home::recompute_prediction(&w); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_set_bedtime_now(move || {
            if let Some(w) = weak.upgrade() {
                use chrono::Local;
                let now = Local::now();
                w.set_bed_hour(now.format("%H").to_string().parse().unwrap_or(22));
                w.set_bed_minute(now.format("%M").to_string().parse().unwrap_or(0));
                home::recompute_prediction(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_set_bedtime_optimal(move || {
            if let Some(w) = weak.upgrade() {
                let sessions = events::get_sessions().unwrap_or_default();
                if sessions.is_empty() { return; }
                let cfg = config::load_config_inner();
                let target = cfg.target_wake_time;
                if let Some(opt) = prediction::find_optimal(&sessions, &home::now_iso(), target.as_deref()) {
                    let parts: Vec<&str> = opt.best_bed_time.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        w.set_bed_hour(parts[0].parse().unwrap_or(22));
                        w.set_bed_minute(parts[1].parse().unwrap_or(0));
                        home::recompute_prediction(&w);
                    }
                }
            }
        });
    }

    // ── 週ナビゲーション ──
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_prev_week(move || {
            if let Some(w) = weak.upgrade() { home::adjust_week(&s, -7); home::update_chart(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_next_week(move || {
            if let Some(w) = weak.upgrade() { home::adjust_week(&s, 7); home::update_chart(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_this_week(move || {
            if let Some(w) = weak.upgrade() { home::reset_week_to_today(&s); home::update_chart(&w, &s); }
        });
    }

    // ── 日別詳細モーダル ──
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_day_clicked(move |date| {
            if let Some(w) = weak.upgrade() { home::open_day_detail(&w, &s, &date); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_close_detail(move || {
            if let Some(w) = weak.upgrade() { home::close_day_detail(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_toggle_add_session(move || {
            if let Some(w) = weak.upgrade() {
                w.set_detail_add_open(!w.get_detail_add_open());
                w.set_detail_error("".into());
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_delete_session(move |start, end| {
            if let Some(w) = weak.upgrade() {
                match events::delete_session(start.to_string(), end.to_string()) {
                    Ok(()) => {
                        home::refresh_all(&w, &s);
                        if let Some(d) = home::selected_date(&s) { home::open_day_detail(&w, &s, &d); }
                    }
                    Err(e) => w.set_detail_error(format!("削除失敗: {}", e).into()),
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_add_session(move || {
            if let Some(w) = weak.upgrade() {
                let Some(date) = home::selected_date(&s) else { return };
                let Ok(d) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else { return };
                let next = d + chrono::Duration::days(1);
                let start = format!("{} {:02}:{:02}:00", d.format("%Y-%m-%d"), w.get_detail_add_start_h(), w.get_detail_add_start_m());
                let end = format!("{} {:02}:{:02}:00", next.format("%Y-%m-%d"), w.get_detail_add_end_h(), w.get_detail_add_end_m());
                if start >= end {
                    w.set_detail_error("起床時刻は入眠時刻より後にしてください".into());
                    return;
                }
                match events::add_session(start, end) {
                    Ok(()) => { home::refresh_all(&w, &s); home::open_day_detail(&w, &s, &date); }
                    Err(e) => w.set_detail_error(format!("追加失敗: {}", e).into()),
                }
            }
        });
    }

    // ── 設定画面 ──
    {
        let weak = window.as_weak();
        window.on_toggle_startup(move |enable| {
            if let Some(w) = weak.upgrade() { settings_ui::toggle_startup(&w, enable); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_create_shortcut(move || {
            if let Some(w) = weak.upgrade() { settings_ui::create_shortcut(&w); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_save_settings(move || {
            if let Some(w) = weak.upgrade() { settings_ui::save(&w); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_test_connection(move || {
            if let Some(w) = weak.upgrade() {
                settings_ui::test_connection(w.as_weak(), w.get_mobile_url().to_string(), w.get_mobile_secret().to_string());
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_sync_now(move || {
            if let Some(w) = weak.upgrade() { settings_ui::sync_now(w.as_weak(), s.clone()); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_export_csv(move || {
            if let Some(w) = weak.upgrade() { settings_ui::export_csv(&w); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_clear_all_data(move || {
            if let Some(w) = weak.upgrade() { settings_ui::clear_all_data(&w, &s); }
        });
    }

    // ── バックグラウンド処理 ──

    // 10秒ごとに現在時刻・起きてから経過時間を更新
    let timer = slint::Timer::default();
    {
        let weak = window.as_weak();
        let s = state.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(10), move || {
            if let Some(w) = weak.upgrade() { home::apply_tick(&w, &s); }
        });
    }

    // 同期ボタン（トップバー）: 別スレッドでsync_gistを実行し、完了後にUIスレッドで再読み込み
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_sync_clicked(move || {
            settings_ui::sync_now(weak.clone(), s.clone());
        });
    }

    // Drive → ローカルへの起動時同期（別スレッド、完了後にUI再読み込み）
    {
        let weak = window.as_weak();
        let s = state.clone();
        std::thread::spawn(move || {
            cloud::ensure_events_from_drive();
            let _ = cloud::pull_mobile_events_inner();
            let weak = weak.clone();
            let s = s.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { home::refresh_all(&w, &s); }
            });
        });
    }

    // アイドル監視スレッド（Windowsデスクトップのみ）
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        let s = state.clone();
        monitor::start(data_dir(), move || {
            let weak = weak.clone();
            let s = s.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { home::refresh_all(&w, &s); }
            });
        });
    }

    window.run().expect("ウィンドウの実行に失敗しました");
}
