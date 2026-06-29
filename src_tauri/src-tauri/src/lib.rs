mod prediction;

use std::path::PathBuf;
use std::process::Command;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct AppConfig {
    pub gist_id: Option<String>,
    pub github_token: Option<String>,
    pub idle_threshold_minutes: Option<u32>,
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
fn save_config(gist_id: String, github_token: String, idle_threshold_minutes: u32) -> Result<(), String> {
    let cfg = AppConfig {
        gist_id: if gist_id.is_empty() { None } else { Some(gist_id) },
        github_token: if github_token.is_empty() { None } else { Some(github_token) },
        idle_threshold_minutes: Some(idle_threshold_minutes),
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

#[tauri::command]
fn sync_gist() -> Result<String, String> {
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

    let url = format!("https://api.github.com/gists/{}", gist_id);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "files": {
            "sleep_events.txt": { "content": content }
        }
    });

    let resp = client
        .patch(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Sleep-Tracker-Tauri/1.0")
        .json(&body)
        .send()
        .map_err(|e| format!("ネットワークエラー: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {} — アップロード失敗", status.as_u16()));
    }

    Ok(format!("同期完了 — {} 行をアップロードしました", line_count))
}

// ── Data management ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_events_content() -> Result<String, String> {
    let path = data_dir().join("sleep_events.txt");
    if !path.exists() { return Ok(String::new()); }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_events(content: String) -> Result<(), String> {
    std::fs::write(data_dir().join("sleep_events.txt"), content).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_all_data() -> Result<(), String> {
    let path = data_dir().join("sleep_events.txt");
    std::fs::write(&path, "").map_err(|e| e.to_string())
}

#[tauri::command]
fn create_desktop_shortcut() -> Result<(), String> {
    // TODO: Phase 5 — create .lnk via Windows Shell API
    Err("デスクトップショートカット作成は未実装です".to_string())
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
    lines.sort_by(|a, b| a[..a.len().min(19)].cmp(&b[..b.len().min(19)]));
    lines.dedup();
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
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

fn repo_root() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let mut dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    for _ in 0..6 {
        if dir.join("src_cpp").exists() {
            return dir;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    std::env::current_dir().unwrap_or_default()
}

fn data_dir() -> PathBuf {
    let dir = repo_root().join("src_tauri").join("data");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[tauri::command]
fn get_sessions() -> Result<Vec<Session>, String> {
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
    lines.push(format!("{},IDLE_START", start));
    lines.push(format!("{},IDLE_RESUME", end));
    lines.sort_by(|a, b| a[..a.len().min(19)].cmp(&b[..b.len().min(19)]));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_session(start: String, end: String) -> Result<(), String> {
    let path = data_dir().join("sleep_events.txt");
    if !path.exists() {
        return Err("sleep_events.txt not found".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let start_line = format!("{},IDLE_START", start);
    let end_line = format!("{},IDLE_RESUME", end);
    let filtered: Vec<&str> = content
        .lines()
        .filter(|l| l.trim() != start_line.as_str() && l.trim() != end_line.as_str())
        .collect();
    std::fs::write(&path, filtered.join("\n") + "\n").map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_sessions, predict_sleep, find_optimal_bedtime,
            add_session, delete_session,
            get_config, save_config, test_github_connection,
            get_startup_enabled, set_startup,
            export_csv, write_csv_file, import_csv,
            get_events_content, restore_events,
            read_text_file, sync_gist, clear_all_data, create_desktop_shortcut,
            get_monitor_status, set_monitor_paused,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
