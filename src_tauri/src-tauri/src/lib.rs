mod prediction;

use std::path::PathBuf;
use std::process::Command;

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

#[tauri::command]
fn get_sessions() -> Result<Vec<Session>, String> {
    let root = repo_root();
    let exe = root.join("src_cpp/parse_sessions.exe");
    let events = root.join("src_cpp/sleep_events.txt");
    let heartbeat = root.join("src_cpp/sleep_heartbeat.txt");
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
    let path = repo_root().join("src_cpp/sleep_events.txt");
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
    lines.push(format!("{} IDLE_START", start));
    lines.push(format!("{} IDLE_RESUME", end));
    lines.sort_by(|a, b| a[..a.len().min(19)].cmp(&b[..b.len().min(19)]));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_session(start: String, end: String) -> Result<(), String> {
    let path = repo_root().join("src_cpp/sleep_events.txt");
    if !path.exists() {
        return Err("sleep_events.txt not found".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let start_line = format!("{} IDLE_START", start);
    let end_line = format!("{} IDLE_RESUME", end);
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
        .invoke_handler(tauri::generate_handler![
            get_sessions, predict_sleep, find_optimal_bedtime,
            add_session, delete_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
