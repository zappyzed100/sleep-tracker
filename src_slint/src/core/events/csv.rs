//! csv.rs — CSVエクスポート・インポート
//!
//! 役割 : セッション一覧のCSV文字列化、ファイル書き出し、CSVからのセッション
//!        取り込み（IDLE_START/IDLE_RESUMEペアとしてsleep_events.txtに追記）。
//!
//! 依存 : super::{Session, TAG, SESSION_CACHE}, super::parsing::sort_events_file

use super::{Session, TAG, SESSION_CACHE};
use super::parsing::sort_events_file;

pub fn export_csv(sessions: &[Session]) -> String {
    let mut out = String::from("就寝時刻,起床時刻,睡眠時間(時間),種別\n");
    for s in sessions {
        out.push_str(&format!("{},{},{:.4},{}\n", s.start, s.end, s.duration_hours, s.session_type));
    }
    out
}

pub fn write_csv_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

pub fn import_csv(csv: String) -> Result<usize, String> {
    let path = crate::data_dir().join("sleep_events.txt");
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
    *SESSION_CACHE.lock().unwrap() = None;
    eprintln!("{} import_csv: {} sessions added", TAG, added);
    Ok(added)
}
