//! lib.rs — Tauri アプリエントリポイント・共有状態・パスユーティリティ
//!
//! 役割 : 共有 static（THRESHOLD_SECS, APP_DIR, SESSION_CACHE）と
//!        パスユーティリティ（data_dir, config_path）、gist_client を定義する。
//!        各サブモジュール（config, events, cloud）を宣言し、
//!        invoke_handler にすべてのコマンドを登録する。
//!        起動・終了処理（setup, on_window_event）もここに集約する。
//!
//! 公開 : `THRESHOLD_SECS`, `SESSION_CACHE`, `data_dir`, `config_path`,
//!        `gist_client`, `run`

mod prediction;
pub mod config;
pub mod events;
pub mod cloud;
#[cfg(not(mobile))]
mod monitor;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// Re-export types used across modules
pub use config::AppConfig;
pub use events::{Session, SessionCache, SESSION_CACHE};

const TAG: &str = "[app]";

// Shared threshold: updated instantly by save_config, read by monitor thread.
pub static THRESHOLD_SECS: AtomicU64 = AtomicU64::new(3600);

// On Android, set to app_data_dir() during setup().
static APP_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(not(mobile))]
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
#[cfg(mobile)]
use tauri::{Manager, WindowEvent};

// ── Path utilities ────────────────────────────────────────────────────────────

// On desktop: walk up from exe to find repo root (contains src_cpp/).
// On Android: not used; data_dir() uses APP_DIR instead.
#[cfg(not(mobile))]
fn repo_root() -> &'static PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        let mut dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        for _ in 0..8 {
            if dir.join("src_cpp").exists() { return dir; }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
        std::env::current_dir().unwrap_or_default()
    })
}

pub(crate) fn data_dir() -> PathBuf {
    // Android: APP_DIR is set during setup() from app.path().app_data_dir()
    #[cfg(mobile)]
    { APP_DIR.get().expect("APP_DIR not initialized").clone() }

    // Desktop: repo_root()/src_tauri/data/
    #[cfg(not(mobile))]
    {
        static DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        DATA.get_or_init(|| {
            let dir = repo_root().join("src_tauri").join("data");
            let _ = std::fs::create_dir_all(&dir);
            dir
        }).clone()
    }
}

pub(crate) fn config_path() -> PathBuf {
    // Android: config lives alongside data
    #[cfg(mobile)]
    { data_dir().join("config.json") }

    // Desktop: config.json at repo root (legacy location, shared with Python)
    #[cfg(not(mobile))]
    { repo_root().join("config.json") }
}

// ── HTTP client ───────────────────────────────────────────────────────────────

pub(crate) fn gist_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

// ── File utils ────────────────────────────────────────────────────────────────

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

// ── Startup / shortcuts (Windows-only) ───────────────────────────────────────

#[cfg(windows)]
const STARTUP_REG_VALUE: &str = "SleepTracker";

#[tauri::command]
fn get_startup_enabled() -> bool {
    #[cfg(windows)] {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        return hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .and_then(|key| key.get_value::<String, _>(STARTUP_REG_VALUE))
            .is_ok();
    }
    #[allow(unreachable_code)]
    false
}

#[tauri::command]
fn set_startup(enable: bool) -> Result<(), String> {
    let _ = enable;
    #[cfg(windows)] {
        use winreg::{RegKey, enums::{HKEY_CURRENT_USER, KEY_WRITE}};
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run = hkcu
            .open_subkey_with_flags("Software\\Microsoft\\Windows\\CurrentVersion\\Run", KEY_WRITE)
            .map_err(|e| e.to_string())?;
        return if enable {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            run.set_value(STARTUP_REG_VALUE, &exe.to_string_lossy().as_ref())
                .map_err(|e| e.to_string())
        } else {
            run.delete_value(STARTUP_REG_VALUE).or(Ok(()))
        };
    }
    #[allow(unreachable_code)]
    Ok(())
}

// ── Monitor status ────────────────────────────────────────────────────────────

fn pause_flag_path() -> PathBuf {
    data_dir().join("monitor_paused")
}

#[tauri::command]
fn get_monitor_status() -> String {
    if pause_flag_path().exists() {
        return "paused".to_string();
    }
    let heartbeat = data_dir().join("sleep_heartbeat.txt");
    if !heartbeat.exists() {
        return "inactive".to_string();
    }
    match heartbeat.metadata().and_then(|m| m.modified()) {
        Ok(t) => {
            let elapsed = t.elapsed().unwrap_or(std::time::Duration::from_secs(9999));
            if elapsed.as_secs() < 90 { "active".to_string() } else { "inactive".to_string() }
        }
        Err(_) => "inactive".to_string(),
    }
}

#[tauri::command]
fn set_monitor_paused(paused: bool) -> Result<(), String> {
    let flag = pause_flag_path();
    if paused {
        std::fs::write(&flag, "").map_err(|e| e.to_string())
    } else {
        if flag.exists() { std::fs::remove_file(&flag).map_err(|e| e.to_string()) } else { Ok(()) }
    }
}

// ── Desktop shortcut ──────────────────────────────────────────────────────────

#[tauri::command]
fn create_desktop_shortcut() -> Result<(), String> {
    #[cfg(windows)] {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let work_dir = exe.parent().unwrap_or(exe.as_path());
        let desktop = desktop_path()?;
        let lnk = desktop.join("睡眠トラッカー.lnk");
        let mut sl = mslnk::ShellLink::new(&exe).map_err(|e| e.to_string())?;
        sl.set_working_dir(Some(work_dir.to_string_lossy().into_owned()));
        sl.set_name(Some("睡眠トラッカー".into()));
        return sl.create_lnk(&lnk).map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    Err("デスクトップショートカットは Windows のみサポートしています".into())
}

#[cfg(windows)]
fn desktop_path() -> Result<PathBuf, String> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders"
    ) {
        if let Ok(path) = key.get_value::<String, _>("Desktop") {
            let p = PathBuf::from(&path);
            if p.exists() { return Ok(p); }
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        for name in &["Desktop", "デスクトップ"] {
            let p = PathBuf::from(&profile).join(name);
            if p.exists() { return Ok(p); }
        }
    }
    Err("デスクトップフォルダが見つかりません".into())
}

// ── Prediction passthrough commands ──────────────────────────────────────────

#[tauri::command]
fn predict_sleep(sessions: Vec<Session>, now_iso: String) -> prediction::PredictionResult {
    prediction::predict(&sessions, &now_iso)
}

#[tauri::command]
fn find_optimal_bedtime(sessions: Vec<Session>, now_iso: String) -> Option<prediction::OptimalResult> {
    let target_wake = config::load_config_inner().target_wake_time;
    prediction::find_optimal(&sessions, &now_iso, target_wake.as_deref())
}

#[tauri::command]
fn is_mobile() -> bool {
    cfg!(mobile)
}

// ── Desktop-only helpers ──────────────────────────────────────────────────────

#[cfg(not(mobile))]
fn show_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

#[cfg(not(mobile))]
fn append_shutdown_event() {
    let path = data_dir().join("sleep_events.txt");
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{},SHUTDOWN", ts);
    }
}

#[cfg(not(mobile))]
fn write_pid() {
    let _ = std::fs::write(data_dir().join("monitor.pid"), std::process::id().to_string());
}

#[cfg(not(mobile))]
fn remove_pid() {
    let _ = std::fs::remove_file(data_dir().join("monitor.pid"));
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // ── Android setup ────────────────────────────────────────────────
            #[cfg(mobile)]
            {
                // Initialize app data dir (must come first, before any data_dir() call)
                let dir = app.path().app_data_dir()
                    .map_err(|e| format!("app_data_dir: {}", e))?;
                let _ = std::fs::create_dir_all(&dir);
                let _ = APP_DIR.set(dir.clone());
                eprintln!("{} setup: platform=android, data_dir={:?}", TAG, dir);
                // Initialize THRESHOLD_SECS from local config (same as desktop setup)
                let init_cfg = config::load_config_inner();
                THRESHOLD_SECS.store(
                    init_cfg.idle_threshold_minutes.unwrap_or(60) as u64 * 60,
                    Ordering::Relaxed,
                );
            }

            // ── Desktop-only setup ───────────────────────────────────────────
            #[cfg(not(mobile))]
            {
                let dd = data_dir();
                eprintln!("{} setup: platform=desktop, data_dir={:?}", TAG, dd);
                write_pid();

                // System tray
                let show_item = MenuItem::with_id(app, "show", "開く", true, None::<&str>)?;
                let sep       = PredefinedMenuItem::separator(app)?;
                let quit_item = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_item, &sep, &quit_item])?;
                TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("睡眠トラッカー")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_window(app),
                        "quit" => {
                            append_shutdown_event();
                            remove_pid();
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event {
                            show_window(tray.app_handle());
                        }
                    })
                    .build(app)?;

                // Initialize shared threshold from config before starting monitor
                let init_cfg = config::load_config_inner();
                THRESHOLD_SECS.store(
                    init_cfg.idle_threshold_minutes.unwrap_or(60) as u64 * 60,
                    Ordering::Relaxed,
                );

                // Start background idle monitor
                monitor::start(data_dir());
            }

            // ── Background thread ────────────────────────────────────────────
            // Android : full bidirectional sync (Drive ↔ local ↔ Sheet) on startup.
            // Desktop : merge Drive → local, pull Sheet events, pre-warm cache,
            //           then poll Sheet every 5 min.
            eprintln!("{} background_thread: started", TAG);
            let t_bg = std::time::Instant::now();
            let bg_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Android: full sync on startup (merge Drive + Sheet, upload merged)
                #[cfg(mobile)]
                {
                    cloud::sync_mobile_inner(); // also updates SESSION_CACHE internally
                    eprintln!("{} background_thread: initial sync done  (+{}ms)", TAG, t_bg.elapsed().as_millis());
                    return;
                }
                // Desktop: one-way startup sync, then emit minute-tick every 60s
                // and pull Sheet events every 5 ticks (5 min).
                #[cfg(not(mobile))]
                {
                    cloud::ensure_events_from_drive();
                    let events_path = data_dir().join("sleep_events.txt");
                    if events_path.exists() {
                        let _ = events::sort_events_file(&events_path);
                    }
                    cloud::pull_mobile_events_inner();
                    let _ = events::parse_sessions_rust().map(|sessions| {
                        let ev_path = data_dir().join("sleep_events.txt");
                        let mtime  = ev_path.metadata().and_then(|m| m.modified())
                                           .unwrap_or(std::time::UNIX_EPOCH);
                        *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions, mtime });
                    });
                    eprintln!("{} background_thread: initial sync done  (+{}ms)", TAG, t_bg.elapsed().as_millis());
                    let mut iter = 0u64;
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(60));
                        // Notify frontend so "起きてから" updates even when window is hidden.
                        let _ = bg_handle.emit("prediction-tick", ());
                        iter += 1;
                        if iter % 5 == 0 {
                            cloud::pull_mobile_events_inner();
                        }
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // Desktop: hide to tray on close instead of quitting
            #[cfg(not(mobile))]
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
            // Suppress unused-variable warning on mobile
            #[cfg(mobile)]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            // events
            events::record_device_on,
            events::get_sessions,
            events::add_session,
            events::delete_session,
            events::get_events_content,
            events::restore_events,
            events::clear_all_data,
            events::export_csv,
            events::write_csv_file,
            events::import_csv,
            // config
            config::get_config,
            config::save_config,
            config::fetch_settings_from_cloud,
            // cloud
            cloud::sync_gist,
            cloud::sync_mobile,
            cloud::fetch_from_cloud,
            cloud::send_screen_on,
            cloud::test_mobile_connection,
            // prediction (via lib.rs wrappers)
            predict_sleep,
            find_optimal_bedtime,
            // platform / lib.rs
            get_startup_enabled,
            set_startup,
            get_monitor_status,
            set_monitor_paused,
            create_desktop_shortcut,
            read_text_file,
            is_mobile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
