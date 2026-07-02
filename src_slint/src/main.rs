//! main.rs — sleep_tracker (Rust + Slint) エントリポイント・共有状態
//!
//! 役割 : Slintウィンドウの起動、共有static（THRESHOLD_SECS, HTTP_CLIENT）、
//!        パスユーティリティ（data_dir, config_path）を定義する。
//!        各モジュール（config, events, cloud, prediction, monitor, platform）を
//!        宣言し、起動時の初期化（Drive同期・monitorスレッド起動）を行う。
//!
//! 公開 : `THRESHOLD_SECS`, `data_dir`, `config_path`, `http_client`

mod config;
mod events;
mod cloud;
mod prediction;
#[cfg(windows)]
mod monitor;
mod platform;

pub use events::Session;

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

    // Drive → ローカルへの起動時同期（別スレッド、UIをブロックしない）
    std::thread::spawn(|| {
        cloud::ensure_events_from_drive();
        let _ = cloud::pull_mobile_events_inner();
    });

    let window = MainWindow::new().expect("ウィンドウの作成に失敗しました");
    window.set_greeting("Rust + Slint 起動成功".into());

    // アイドル監視スレッド（Windowsデスクトップのみ）
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        monitor::start(data_dir(), move || {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(_w) = weak.upgrade() {
                    // TODO: セッション一覧の再読み込みコールバックをUI側に実装したら呼ぶ
                }
            });
        });
    }

    window.run().expect("ウィンドウの実行に失敗しました");
}
