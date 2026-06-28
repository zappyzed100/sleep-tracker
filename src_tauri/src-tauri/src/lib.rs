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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_sessions, predict_sleep, find_optimal_bedtime])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
