mod prediction;
mod monitor;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct AppConfig {
    pub gist_id: Option<String>,
    pub github_token: Option<String>,
    pub idle_threshold_minutes: Option<u32>,
    pub mobile_url: Option<String>,
    pub mobile_secret: Option<String>,
}

fn config_path() -> PathBuf {
    repo_root().join("config.json")
}

fn load_config_inner() -> AppConfig {
    let path = config_path();
    if !path.exists() { return AppConfig::default(); }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn get_config() -> AppConfig {
    load_config_inner()
}

#[tauri::command]
fn save_config(
    gist_id: String,
    github_token: String,
    idle_threshold_minutes: u32,
    mobile_url: String,
    mobile_secret: String,
) -> Result<(), String> {
    let cfg = AppConfig {
        gist_id: if gist_id.is_empty() { None } else { Some(gist_id) },
        github_token: if github_token.is_empty() { None } else { Some(github_token) },
        idle_threshold_minutes: Some(idle_threshold_minutes),
        mobile_url: if mobile_url.is_empty() { None } else { Some(mobile_url) },
        mobile_secret: if mobile_secret.is_empty() { None } else { Some(mobile_secret) },
    };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())
}

#[tauri::command]
fn test_github_connection(gist_id: String, github_token: String) -> Result<String, String> {
    if gist_id.is_empty() || github_token.is_empty() {
        return Err("Gist ID とトークンを入力してください".to_string());
    }
    let url = format!("https://api.github.com/gists/{}", gist_id);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", github_token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Sleep-Tracker-Tauri/1.0")
        .send()
        .map_err(|e| format!("ネットワークエラー: {}", e))?;
    let status = resp.status();
    if status.is_success() {
        Ok(format!("接続成功 (HTTP {})", status.as_u16()))
    } else {
        Err(format!("HTTP {} — {}", status.as_u16(),
            match status.as_u16() {
                401 => "認証失敗（トークンを確認）",
                404 => "Gist が見つかりません（IDを確認）",
                _ => status.canonical_reason().unwrap_or("エラー"),
            }
        ))
    }
}

// ── Startup / shortcuts ───────────────────────────────────────────────────────

const STARTUP_REG_VALUE: &str = "SleepTracker";

#[tauri::command]
fn get_startup_enabled() -> bool {
    use winreg::{RegKey, enums::*};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .and_then(|key| key.get_value::<String, _>(STARTUP_REG_VALUE))
        .is_ok()
}

#[tauri::command]
fn set_startup(enable: bool) -> Result<(), String> {
    use winreg::{RegKey, enums::*};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu
        .open_subkey_with_flags("Software\\Microsoft\\Windows\\CurrentVersion\\Run", KEY_WRITE)
        .map_err(|e| e.to_string())?;
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        run.set_value(STARTUP_REG_VALUE, &exe.to_string_lossy().as_ref())
            .map_err(|e| e.to_string())
    } else {
        run.delete_value(STARTUP_REG_VALUE).or(Ok(()))
    }
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

// ── File utils ───────────────────────────────────────────────────────────────

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

// ── Gist sync ─────────────────────────────────────────────────────────────────

fn gist_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn gist_headers(token: &str) -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("Authorization", format!("Bearer {}", token).parse().unwrap());
    h.insert("Accept", "application/vnd.github+json".parse().unwrap());
    h.insert("User-Agent", "Sleep-Tracker-Tauri/1.0".parse().unwrap());
    h
}

// Parse and apply one "TAG,TIMESTAMP" line from mobile_event.txt.
fn apply_mobile_event_line(line: &str) -> Result<String, String> {
    let mut parts = line.splitn(2, ',');
    let tag      = parts.next().ok_or("フォーマット不正")?.trim();
    let time_raw = parts.next().ok_or("フォーマット不正")?.trim();

    let event_type = match tag {
        "LEAVE_HOME" | "LEAVE"   => "OUT_START",
        "ARRIVE_HOME" | "ARRIVE" => "OUT_END",
        "SCREEN_ON"              => "DEVICE_ON",
        other                    => return Err(format!("不明タグ: {}", other)),
    };

    let ts = if let Ok(ms) = time_raw.parse::<i64>() {
        use chrono::{Local, TimeZone};
        Local.timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| time_raw.to_string())
    } else {
        time_raw.to_string()
    };

    let new_line    = format!("{},{}", ts, event_type);
    let events_path = data_dir().join("sleep_events.txt");

    if events_path.exists() {
        let existing = std::fs::read_to_string(&events_path).unwrap_or_default();
        if existing.lines().any(|l| l.trim() == new_line.as_str()) {
            return Ok(format!("重複スキップ: {}", new_line));
        }
    }

    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&events_path) {
        let _ = writeln!(f, "{}", new_line);
    } else {
        return Err("書き込み失敗".into());
    }

    if event_type == "DEVICE_ON" {
        let _ = std::fs::write(data_dir().join("device_heartbeat.txt"), format!("{}\n", ts));
    }

    Ok(format!("追加: {}", new_line))
}

// Pull events from Google Apps Script Web App (GET returns all rows as TAG,TIMESTAMP lines).
// Duplicate check in apply_mobile_event_line skips already-processed events.
fn pull_mobile_events_inner() -> String {
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "スキップ (設定なし)".into(),
    };

    let client = match gist_client() {
        Ok(c) => c,
        Err(e) => return format!("クライアントエラー: {}", e),
    };

    let url = format!("{}?secret={}", base_url.trim_end_matches('/'), secret);
    let resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => return format!("取得失敗: {}", e),
    };
    if !resp.status().is_success() {
        return format!("HTTP {}", resp.status().as_u16());
    }
    let content = match resp.text() {
        Ok(t) => t.trim().to_string(),
        Err(e) => return format!("レスポンス読み取り失敗: {}", e),
    };

    if content.is_empty() || content == "Unauthorized" {
        if content == "Unauthorized" { return "認証失敗（シークレットを確認）".into(); }
        return "モバイルイベントなし".into();
    }

    let mut msgs = Vec::new();
    let mut new_events = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        match apply_mobile_event_line(line) {
            Ok(msg) => {
                if msg.starts_with("追加") { new_events += 1; }
                msgs.push(msg);
            }
            Err(e) => msgs.push(format!("エラー: {}", e)),
        }
    }

    if new_events > 0 {
        let events_path = data_dir().join("sleep_events.txt");
        let _ = sort_events_file(&events_path);
        *SESSION_CACHE.lock().unwrap() = None;
    }

    if msgs.is_empty() {
        return "モバイルイベントなし".into();
    }

    format!("{} 件処理: {}", msgs.len(), msgs.join(" / "))
}

#[tauri::command]
fn test_mobile_connection(mobile_url: String, mobile_secret: String) -> Result<String, String> {
    if mobile_url.is_empty() || mobile_secret.is_empty() {
        return Err("URL とシークレットを入力してください".to_string());
    }
    let url = format!("{}?secret={}&action=health", mobile_url.trim_end_matches('/'), mobile_secret);
    let resp = gist_client()?
        .get(&url)
        .send()
        .map_err(|e| format!("ネットワークエラー: {}", e))?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status.is_success() && body.trim() == "ok" {
        Ok("接続成功".to_string())
    } else if body.trim() == "Unauthorized" {
        Err("認証失敗（シークレットを確認）".to_string())
    } else {
        Err(format!("HTTP {} — レスポンス: {}", status.as_u16(), body.trim()))
    }
}

#[tauri::command]
fn sync_gist() -> Result<String, String> {
    // 1. Pull mobile events from Gist
    let pull_msg = pull_mobile_events_inner();

    // 2. Push sleep_events.txt to Gist
    let cfg = load_config_inner();
    let gist_id = cfg.gist_id.ok_or_else(|| "Gist ID が設定されていません".to_string())?;
    let token = cfg.github_token.ok_or_else(|| "GitHub Token が設定されていません".to_string())?;

    let events_path = data_dir().join("sleep_events.txt");
    let content = if events_path.exists() {
        std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let line_count = content.lines().count();

    let body = serde_json::json!({
        "files": { "sleep_events.txt": { "content": content } }
    });

    let resp = gist_client()?
        .patch(format!("https://api.github.com/gists/{}", gist_id))
        .headers(gist_headers(&token))
        .json(&body)
        .send()
        .map_err(|e| format!("ネットワークエラー: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} — アップロード失敗", resp.status().as_u16()));
    }

    Ok(format!("同期完了 — {} 行アップロード / モバイル: {}", line_count, pull_msg))
}

// ── Data management ───────────────────────────────────────────────────────────

fn sort_events_file(path: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = content.lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    lines.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));
    lines.dedup();
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

fn ensure_events_from_gist() {
    let path = data_dir().join("sleep_events.txt");
    if path.exists() { return; }

    let cfg = load_config_inner();
    let (gist_id, token) = match (cfg.gist_id, cfg.github_token) {
        (Some(g), Some(t)) if !g.is_empty() && !t.is_empty() => (g, t),
        _ => return,
    };
    let client = match gist_client() { Ok(c) => c, Err(_) => return };
    let resp = match client
        .get(format!("https://api.github.com/gists/{}", gist_id))
        .headers(gist_headers(&token))
        .send()
    {
        Ok(r) if r.status().is_success() => r,
        _ => return,
    };
    let json: serde_json::Value = match resp.json() { Ok(j) => j, Err(_) => return };
    let content = json["files"]["sleep_events.txt"]["content"]
        .as_str().unwrap_or("").to_string();
    if !content.is_empty() {
        let _ = std::fs::write(&path, content);
    }
}

#[tauri::command]
fn get_events_content() -> Result<String, String> {
    let path = data_dir().join("sleep_events.txt");
    if !path.exists() { return Ok(String::new()); }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_events(content: String) -> Result<(), String> {
    let path = data_dir().join("sleep_events.txt");
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    sort_events_file(&path)
}

#[tauri::command]
fn clear_all_data() -> Result<(), String> {
    let path = data_dir().join("sleep_events.txt");
    std::fs::write(&path, "").map_err(|e| e.to_string())
}

#[tauri::command]
fn create_desktop_shortcut() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let work_dir = exe.parent().unwrap_or(exe.as_path());

    // Resolve Desktop folder — handles OneDrive redirection and locale variations
    let desktop = desktop_path()?;
    let lnk = desktop.join("睡眠トラッカー.lnk");

    let mut sl = mslnk::ShellLink::new(&exe).map_err(|e| e.to_string())?;
    sl.set_working_dir(Some(work_dir.to_string_lossy().into_owned()));
    sl.set_name(Some("睡眠トラッカー".into()));
    sl.create_lnk(&lnk).map_err(|e| e.to_string())
}

fn desktop_path() -> Result<PathBuf, String> {
    use winreg::{RegKey, enums::*};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    // "Shell Folders" contains already-expanded paths (most reliable)
    if let Ok(key) = hkcu.open_subkey(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders"
    ) {
        if let Ok(path) = key.get_value::<String, _>("Desktop") {
            let p = PathBuf::from(&path);
            if p.exists() { return Ok(p); }
        }
    }
    // Fallback: derive from USERPROFILE
    if let Ok(profile) = std::env::var("USERPROFILE") {
        for name in &["Desktop", "デスクトップ"] {
            let p = PathBuf::from(&profile).join(name);
            if p.exists() { return Ok(p); }
        }
    }
    Err("デスクトップフォルダが見つかりません".into())
}

// ── CSV export ────────────────────────────────────────────────────────────────

#[tauri::command]
fn export_csv(sessions: Vec<Session>) -> String {
    let mut out = String::from("就寝時刻,起床時刻,睡眠時間(時間),種別\n");
    for s in &sessions {
        out.push_str(&format!("{},{},{:.4},{}\n", s.start, s.end, s.duration_hours, s.session_type));
    }
    out
}

#[tauri::command]
fn write_csv_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_csv(csv: String) -> Result<usize, String> {
    let path = data_dir().join("sleep_events.txt");
    let existing = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();

    let mut added = 0usize;
    for line in csv.lines().skip(1) {  // skip header
        let cols: Vec<&str> = line.splitn(4, ',').collect();
        if cols.len() < 2 { continue; }
        let start = cols[0].trim();
        let end = cols[1].trim();
        if start.len() < 19 || end.len() < 19 { continue; }
        lines.push(format!("{},IDLE_START", start));
        lines.push(format!("{},IDLE_RESUME", end));
        added += 1;
    }
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    sort_events_file(&path)?;
    Ok(added)
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub start: String,
    pub end: String,
    #[serde(rename = "duration")]
    pub duration_hours: f64,
    #[serde(rename = "type")]
    pub session_type: String,
}

fn repo_root() -> &'static PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        let mut dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        for _ in 0..8 {
            if dir.join("src_cpp").exists() {
                return dir;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
        std::env::current_dir().unwrap_or_default()
    })
}

fn data_dir() -> PathBuf {
    static DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        let dir = repo_root().join("src_tauri").join("data");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }).clone()
}

struct SessionCache {
    sessions: Vec<Session>,
    mtime: std::time::SystemTime,
}
static SESSION_CACHE: std::sync::Mutex<Option<SessionCache>> = std::sync::Mutex::new(None);

fn run_parse_sessions() -> Result<Vec<Session>, String> {
    let root = repo_root();
    let exe = root.join("src_cpp/parse_sessions.exe");
    let events = data_dir().join("sleep_events.txt");
    let heartbeat = data_dir().join("sleep_heartbeat.txt");
    let config = root.join("config.json");

    if !exe.exists() {
        return Err(format!("parse_sessions.exe not found at {:?}", exe));
    }

    let out = Command::new(&exe)
        .arg(&events)
        .arg(&heartbeat)
        .arg(&config)
        .output()
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }

    let json = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<Vec<Session>>(&json).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_sessions() -> Result<Vec<Session>, String> {
    let events = data_dir().join("sleep_events.txt");
    let current_mtime = events.metadata()
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH);

    let mut cache = SESSION_CACHE.lock().unwrap();
    if let Some(c) = cache.as_ref() {
        if c.mtime == current_mtime {
            return Ok(c.sessions.clone());
        }
    }

    let sessions = run_parse_sessions()?;
    *cache = Some(SessionCache { sessions: sessions.clone(), mtime: current_mtime });
    Ok(sessions)
}

#[tauri::command]
fn predict_sleep(sessions: Vec<Session>, now_iso: String) -> prediction::PredictionResult {
    prediction::predict(&sessions, &now_iso)
}

#[tauri::command]
fn find_optimal_bedtime(sessions: Vec<Session>, now_iso: String) -> Option<prediction::OptimalResult> {
    prediction::find_optimal(&sessions, &now_iso)
}

#[tauri::command]
fn add_session(start: String, end: String) -> Result<(), String> {
    let path = data_dir().join("sleep_events.txt");
    let mut content = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    content.push_str(&format!("{},IDLE_START\n{},IDLE_RESUME\n", start, end));
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    sort_events_file(&path)
}

#[tauri::command]
fn delete_session(start: String, end: String) -> Result<(), String> {
    let path = data_dir().join("sleep_events.txt");
    if !path.exists() {
        return Err("sleep_events.txt not found".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let start_line = format!("{},IDLE_START", start);
    let end_line   = format!("{},IDLE_RESUME", end);
    let filtered: String = content
        .lines()
        .filter(|l| l.trim() != start_line.as_str() && l.trim() != end_line.as_str())
        .flat_map(|l| [l, "\n"])
        .collect();
    std::fs::write(&path, &filtered).map_err(|e| e.to_string())?;
    sort_events_file(&path)
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn append_shutdown_event() {
    let path = data_dir().join("sleep_events.txt");
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{},SHUTDOWN", ts);
    }
}

fn write_pid() {
    let _ = std::fs::write(data_dir().join("monitor.pid"), std::process::id().to_string());
}

fn remove_pid() {
    let _ = std::fs::remove_file(data_dir().join("monitor.pid"));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // PID file
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

            // Background startup: restore from Gist if missing → pull mobile →
            // pre-warm cache → auto-pull every 5 min
            std::thread::spawn(|| {
                ensure_events_from_gist();
                pull_mobile_events_inner();
                let _ = run_parse_sessions().map(|sessions| {
                    let events = data_dir().join("sleep_events.txt");
                    let mtime  = events.metadata().and_then(|m| m.modified())
                                       .unwrap_or(std::time::UNIX_EPOCH);
                    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions, mtime });
                });
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(300));
                    pull_mobile_events_inner();
                }
            });

            // Start background monitor
            monitor::start(data_dir(), config_path());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Minimize to tray instead of quitting
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_sessions, predict_sleep, find_optimal_bedtime,
            add_session, delete_session,
            get_config, save_config, test_github_connection, test_mobile_connection,
            get_startup_enabled, set_startup,
            export_csv, write_csv_file, import_csv,
            get_events_content, restore_events,
            read_text_file, sync_gist, clear_all_data, create_desktop_shortcut,
            get_monitor_status, set_monitor_paused,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
