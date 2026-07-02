//! main.rs — sleep_tracker (Rust + Slint) エントリポイント・共有状態
//!
//! 役割 : Slintウィンドウの起動、共有static（THRESHOLD_SECS, HTTP_CLIENT）、
//!        パスユーティリティ（data_dir, config_path）を定義する。
//!        各モジュール（config, events, cloud, prediction, monitor, platform）を
//!        宣言し、起動時の初期化（Drive同期・monitorスレッド起動）と
//!        ホーム画面の統計ストリップへのデータバインディングを行う。
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

use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

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

// ── 統計ストリップ用の状態 ────────────────────────────────────────────────────
//
// StatsCard.tsx 相当。awake_hours はUIスレッドのTimerで10秒ごとに経過時間を
// 加算して表示するだけにし、毎回 predict() を呼び直さない（軽量化）。

struct StatsBaseline {
    awake_hours: f64,
    computed_at: Instant,
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

// StatsCard.tsx の期間タブ「先月」(30日) 相当をデフォルトとして使用。
fn compute_and_apply(window: &MainWindow, baseline: &Arc<Mutex<Option<StatsBaseline>>>) {
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
    *baseline.lock().unwrap() = Some(StatsBaseline { awake_hours: pred.awake_hours, computed_at: Instant::now() });

    apply_tick(window, baseline);
}

// 現在時刻・起きてから経過時間だけを軽量に更新する（10秒ごと）。
fn apply_tick(window: &MainWindow, baseline: &Arc<Mutex<Option<StatsBaseline>>>) {
    window.set_current_time(now_hhmm().into());
    if let Some(b) = baseline.lock().unwrap().as_ref() {
        let elapsed_h = b.computed_at.elapsed().as_secs_f64() / 3600.0;
        let awake = b.awake_hours + elapsed_h;
        window.set_awake_since(utils::format_duration(awake).into());
        window.set_awake_color(awake_color(awake));
    }
}

fn main() {
    // 起動時初期化: config.jsonからTHRESHOLD_SECSを読み込み
    let cfg = config::load_config_inner();
    if let Some(m) = cfg.idle_threshold_minutes {
        THRESHOLD_SECS.store(m as u64 * 60, Ordering::Relaxed);
    }

    let window = MainWindow::new().expect("ウィンドウの作成に失敗しました");
    window.set_greeting("Rust + Slint 起動成功".into());

    let baseline: Arc<Mutex<Option<StatsBaseline>>> = Arc::new(Mutex::new(None));
    compute_and_apply(&window, &baseline);

    // 10秒ごとに現在時刻・起きてから経過時間を更新
    let timer = slint::Timer::default();
    {
        let weak = window.as_weak();
        let baseline_for_timer = baseline.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(10), move || {
            if let Some(w) = weak.upgrade() {
                apply_tick(&w, &baseline_for_timer);
            }
        });
    }

    // 同期ボタン: 別スレッドでsync_gistを実行し、完了後にUIスレッドで再読み込み
    {
        let weak = window.as_weak();
        let baseline_for_sync = baseline.clone();
        window.on_sync_clicked(move || {
            let weak = weak.clone();
            let baseline_for_sync = baseline_for_sync.clone();
            std::thread::spawn(move || {
                let msg = cloud::sync_gist();
                eprintln!("[app] sync_gist: {:?}", msg);
                let weak = weak.clone();
                let baseline_for_sync = baseline_for_sync.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        compute_and_apply(&w, &baseline_for_sync);
                    }
                });
            });
        });
    }

    // Drive → ローカルへの起動時同期（別スレッド、完了後にUI再読み込み）
    {
        let weak = window.as_weak();
        let baseline_for_startup = baseline.clone();
        std::thread::spawn(move || {
            cloud::ensure_events_from_drive();
            let _ = cloud::pull_mobile_events_inner();
            let weak = weak.clone();
            let baseline_for_startup = baseline_for_startup.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    compute_and_apply(&w, &baseline_for_startup);
                }
            });
        });
    }

    // アイドル監視スレッド（Windowsデスクトップのみ）
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        let baseline_for_monitor = baseline.clone();
        monitor::start(data_dir(), move || {
            let weak = weak.clone();
            let baseline_for_monitor = baseline_for_monitor.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    compute_and_apply(&w, &baseline_for_monitor);
                }
            });
        });
    }

    window.run().expect("ウィンドウの実行に失敗しました");
}
