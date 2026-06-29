mod prediction;
#[cfg(not(mobile))]
mod monitor;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// Shared threshold: updated instantly by save_config, read by monitor thread.
pub static THRESHOLD_SECS: AtomicU64 = AtomicU64::new(3600);

// On Android, set to app_data_dir() during setup().
static APP_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(not(mobile))]
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
#[cfg(mobile)]
use tauri::{Manager, WindowEvent};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct AppConfig {
    pub idle_threshold_minutes: Option<u32>,
    pub mobile_url: Option<String>,
    pub mobile_secret: Option<String>,
    pub target_wake_time: Option<String>,
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
    idle_threshold_minutes: u32,
    mobile_url: String,
    mobile_secret: String,
    target_wake_time: Option<String>,
) -> Result<(), String> {
    let cfg = AppConfig {
        idle_threshold_minutes: Some(idle_threshold_minutes),
        mobile_url: if mobile_url.is_empty() { None } else { Some(mobile_url) },
        mobile_secret: if mobile_secret.is_empty() { None } else { Some(mobile_secret) },
        target_wake_time: target_wake_time.filter(|s| !s.is_empty()),
    };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())?;
    THRESHOLD_SECS.store(idle_threshold_minutes as u64 * 60, Ordering::Relaxed);
    *SESSION_CACHE.lock().unwrap() = None;
    Ok(())
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

fn backup_to_drive(content: &str) -> String {
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Driveスキップ(未設定)".into(),
    };

    let url = format!("{}?secret={}&action=backup", base_url.trim_end_matches('/'), secret);
    let resp = match gist_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => return format!("Drive送信失敗: {}", e),
    };

    if resp.status().is_success() {
        "Drive バックアップ完了".into()
    } else {
        format!("Drive HTTP {}", resp.status().as_u16())
    }
}

#[tauri::command]
fn sync_gist() -> Result<String, String> {
    // 1. Pull mobile events from Google Sheets
    let pull_msg = pull_mobile_events_inner();

    // 2. Always sort+dedup the file (removes duplicate IDLE_START lines caused
    //    by monitor oscillation when threshold < WAKE_SECS)
    let events_path = data_dir().join("sleep_events.txt");
    if events_path.exists() {
        let _ = sort_events_file(&events_path);
    }

    // 3. Read updated sleep_events.txt
    let content = if events_path.exists() {
        std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };

    // 4. Backup to Google Drive via Apps Script
    let drive_msg = backup_to_drive(&content);

    Ok(format!("同期完了 — モバイル: {} / {}", pull_msg, drive_msg))
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

fn ensure_events_from_drive() {
    let path = data_dir().join("sleep_events.txt");
    if path.exists() { return; }

    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return,
    };
    let client = match gist_client() { Ok(c) => c, Err(_) => return };
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let resp = match client.get(&url).send() {
        Ok(r) if r.status().is_success() => r,
        _ => return,
    };
    let content = match resp.text() {
        Ok(t) if !t.trim().is_empty() => t,
        _ => return,
    };
    let _ = std::fs::write(&path, content);
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

fn data_dir() -> PathBuf {
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

fn config_path() -> PathBuf {
    // Android: config lives alongside data
    #[cfg(mobile)]
    { data_dir().join("config.json") }

    // Desktop: config.json at repo root (legacy location, shared with Python)
    #[cfg(not(mobile))]
    { repo_root().join("config.json") }
}

struct SessionCache {
    sessions: Vec<Session>,
    mtime: std::time::SystemTime,
}
static SESSION_CACHE: std::sync::Mutex<Option<SessionCache>> = std::sync::Mutex::new(None);

fn parse_sessions_rust() -> Result<Vec<Session>, String> {
    use chrono::{NaiveDateTime, TimeZone, Local};

    let events_path   = data_dir().join("sleep_events.txt");
    let heartbeat_path = data_dir().join("sleep_heartbeat.txt");
    let min_sleep_secs = THRESHOLD_SECS.load(Ordering::Relaxed) as i64;

    let ts_to_epoch = |s: &str| -> Option<i64> {
        let ndt = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
        Local.from_local_datetime(&ndt).earliest().map(|d| d.timestamp())
    };

    let epoch_to_ts = |ep: i64| -> String {
        Local.timestamp_opt(ep, 0)
            .single()
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default()
    };

    // Heartbeat (for POWER_LOSS start-time correction)
    let mut hb_epoch: i64 = 0;
    let mut hb_idle_ms: i64 = 0;
    if let Ok(txt) = std::fs::read_to_string(&heartbeat_path) {
        if let Some(line) = txt.lines().next() {
            let line = line.trim_end_matches('\r');
            if let Some(c) = line.find(',') {
                if let Some(ep) = ts_to_epoch(&line[..c]) {
                    hb_epoch = ep;
                    hb_idle_ms = line[c+1..].trim().parse().unwrap_or(0);
                }
            }
        }
    }

    // Events
    if !events_path.exists() { return Ok(vec![]); }
    let raw = std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?;

    struct Ev { epoch: i64, ts: String, ty: String }
    let mut evs: Vec<Ev> = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() { continue; }
        if let Some(c) = line.find(',') {
            if let Some(ep) = ts_to_epoch(&line[..c]) {
                evs.push(Ev { epoch: ep, ts: line[..c].to_string(), ty: line[c+1..].to_string() });
            }
        }
    }
    evs.sort_by_key(|e| e.epoch);

    // State machine (mirrors parse_sessions.cpp logic)
    let mut sessions: Vec<Session> = Vec::new();
    let mut sleeping = false;
    let mut sleep_start_ep: i64 = 0;
    let mut sleep_start_ts = String::new();
    let mut session_type = String::new();
    let mut is_out = false;

    let push = |sessions: &mut Vec<Session>, start_ts: &str, start_ep: i64,
                end_ts: &str, end_ep: i64, stype: &str| {
        let dur = end_ep - start_ep;
        if dur >= min_sleep_secs {
            sessions.push(Session {
                start: start_ts.to_string(),
                end: end_ts.to_string(),
                duration_hours: dur as f64 / 3600.0,
                session_type: stype.to_string(),
            });
        }
    };

    let n = evs.len();
    for i in 0..n {
        let (ep, ts, ty) = (evs[i].epoch, evs[i].ts.as_str(), evs[i].ty.as_str());

        if ty == "DEVICE_ON" {
            if sleeping { push(&mut sessions, &sleep_start_ts, sleep_start_ep, ts, ep, &session_type); sleeping = false; }
            continue;
        }
        if ty == "OUT_START" {
            is_out = true;
            if sleeping { push(&mut sessions, &sleep_start_ts, sleep_start_ep, ts, ep, &session_type); sleeping = false; }
            continue;
        }
        if ty == "OUT_END" { is_out = false; continue; }

        if !sleeping {
            if !is_out && matches!(ty, "IDLE_START" | "SUSPEND" | "SHUTDOWN") {
                sleeping = true;
                sleep_start_ep = ep;
                sleep_start_ts = ts.to_string();
                session_type = if ty == "IDLE_START" { "IDLE" } else { "POWER" }.to_string();
            } else if matches!(ty, "STARTUP" | "RESUME") && i > 0 {
                if !is_out && ep - evs[i-1].epoch > 4 * 3600 {
                    let prev_ep = evs[i-1].epoch;
                    let (start_ep, start_ts_s) = if hb_epoch > 0 && hb_epoch > prev_ep && hb_epoch < ep {
                        let adj = hb_epoch - hb_idle_ms / 1000;
                        if adj > prev_ep { (adj, epoch_to_ts(adj)) } else { (prev_ep, evs[i-1].ts.clone()) }
                    } else {
                        (prev_ep, evs[i-1].ts.clone())
                    };
                    push(&mut sessions, &start_ts_s, start_ep, ts, ep, "POWER_LOSS");
                }
            }
        } else {
            if matches!(ty, "IDLE_RESUME" | "RESUME" | "STARTUP") {
                push(&mut sessions, &sleep_start_ts, sleep_start_ep, ts, ep, &session_type);
                sleeping = false;
            } else if matches!(ty, "SUSPEND" | "SHUTDOWN") {
                session_type = "POWER".to_string();
            }
        }
    }

    Ok(sessions)
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

    let sessions = parse_sessions_rust()?;
    *cache = Some(SessionCache { sessions: sessions.clone(), mtime: current_mtime });
    Ok(sessions)
}

#[tauri::command]
fn predict_sleep(sessions: Vec<Session>, now_iso: String) -> prediction::PredictionResult {
    prediction::predict(&sessions, &now_iso)
}

#[tauri::command]
fn find_optimal_bedtime(sessions: Vec<Session>, now_iso: String) -> Option<prediction::OptimalResult> {
    let target_wake = load_config_inner().target_wake_time;
    prediction::find_optimal(&sessions, &now_iso, target_wake.as_deref())
}

#[tauri::command]
fn is_mobile() -> bool {
    cfg!(mobile)
}

// Android: fetch sleep_events.txt from Drive and rebuild local session cache.
// Does NOT push anything back — read-only from Android's perspective.
#[tauri::command]
fn fetch_from_cloud() -> Result<Vec<Session>, String> {
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let client = gist_client()?;
    let resp = client.get(&url).send().map_err(|e| format!("取得失敗: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let content = resp.text().map_err(|e| format!("レスポンス読み取り失敗: {}", e))?;
    if content.trim().is_empty() || content.trim() == "Unauthorized" {
        if content.trim() == "Unauthorized" {
            return Err("認証失敗（シークレットを確認）".into());
        }
        return Ok(vec![]);
    }
    // Write locally so parse_sessions_rust() can read it
    let path = data_dir().join("sleep_events.txt");
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    // Invalidate and rebuild cache
    *SESSION_CACHE.lock().unwrap() = None;
    let sessions = parse_sessions_rust()?;
    let mtime = path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions: sessions.clone(), mtime });
    Ok(sessions)
}

// Send a SCREEN_ON event to the Apps Script (Android: called every 5 min while app is active).
// The Apps Script stores it in mobile_event.txt; the Windows app picks it up on next sync.
#[tauri::command]
fn send_screen_on() -> Result<String, String> {
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    let ts = chrono::Local::now().timestamp_millis();
    let url = format!("{}?secret={}", base_url.trim_end_matches('/'), secret);
    let client = gist_client()?;
    let resp = client
        .post(&url)
        .header("Content-Type", "text/plain")
        .body(format!("SCREEN_ON,{}", ts))
        .send()
        .map_err(|e| format!("送信失敗: {}", e))?;
    if resp.status().is_success() {
        Ok("SCREEN_ON 送信完了".into())
    } else {
        Err(format!("HTTP {}", resp.status().as_u16()))
    }
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
                let _ = APP_DIR.set(dir);
            }

            // ── Desktop-only setup ───────────────────────────────────────────
            #[cfg(not(mobile))]
            {
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
                let init_cfg = load_config_inner();
                THRESHOLD_SECS.store(
                    init_cfg.idle_threshold_minutes.unwrap_or(60) as u64 * 60,
                    Ordering::Relaxed,
                );

                // Start background idle monitor
                monitor::start(data_dir());
            }

            // ── Shared background thread (both platforms) ────────────────────
            // Restore from Drive if sleep_events.txt missing → pull mobile events
            // → pre-warm session cache → auto-pull every 5 min (desktop only)
            std::thread::spawn(|| {
                ensure_events_from_drive();
                let events_path = data_dir().join("sleep_events.txt");
                if events_path.exists() {
                    let _ = sort_events_file(&events_path);
                }
                pull_mobile_events_inner();
                let _ = parse_sessions_rust().map(|sessions| {
                    let events = data_dir().join("sleep_events.txt");
                    let mtime  = events.metadata().and_then(|m| m.modified())
                                       .unwrap_or(std::time::UNIX_EPOCH);
                    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions, mtime });
                });
                // Desktop: keep auto-pulling. Android: the frontend's SCREEN_ON
                // timer and manual sync button handle freshness.
                #[cfg(not(mobile))]
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(300));
                    pull_mobile_events_inner();
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
            get_sessions, predict_sleep, find_optimal_bedtime,
            add_session, delete_session,
            get_config, save_config, test_mobile_connection,
            get_startup_enabled, set_startup,
            export_csv, write_csv_file, import_csv,
            get_events_content, restore_events,
            read_text_file, sync_gist, clear_all_data, create_desktop_shortcut,
            get_monitor_status, set_monitor_paused,
            is_mobile, send_screen_on, fetch_from_cloud,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
