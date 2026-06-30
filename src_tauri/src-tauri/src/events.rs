//! events.rs — sleep_events.txt のパース・セッション管理
//!
//! 役割 : sleep_events.txt を読み込んで Session に変換するステートマシン実装。
//!        セッションの追加・削除・エクスポートなど全データ操作を担当する。
//!        SESSION_CACHE でパース結果をキャッシュしパフォーマンスを確保する。
//!
//! 依存 : crate::data_dir, crate::THRESHOLD_SECS, chrono
//! 公開 : `Session`, `SessionCache`, `SESSION_CACHE`, `parse_sessions_rust`,
//!        `sort_events_file`, `get_sessions`, `add_session`, `delete_session`,
//!        `get_events_content`, `restore_events`, `clear_all_data`,
//!        `export_csv`, `write_csv_file`, `import_csv`,
//!        `is_out_from_content`, `apply_mobile_event_line`

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

const TAG: &str = "[events]";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub start: String,
    pub end: String,
    #[serde(rename = "duration")]
    pub duration_hours: f64,
    #[serde(rename = "type")]
    pub session_type: String,
}

pub struct SessionCache {
    pub sessions: Vec<Session>,
    pub mtime: std::time::SystemTime,
}

pub static SESSION_CACHE: std::sync::Mutex<Option<SessionCache>> = std::sync::Mutex::new(None);

// Returns true if the last OUT_START in the file has no matching OUT_END / IN_HOUSE.
pub fn is_out_from_content(content: &str) -> bool {
    let mut out = false;
    for line in content.lines() {
        if let Some(c) = line.trim().find(',') {
            match &line.trim()[c + 1..] {
                "OUT_START" => out = true,
                "OUT_END" | "IN_HOUSE" => out = false,
                _ => {}
            }
        }
    }
    out
}

pub fn sort_events_file(path: &std::path::Path) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = content.lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    lines.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));
    lines.dedup();
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    let ms = t0.elapsed().as_millis();
    if ms > 100 {
        eprintln!("{} sort: SLOW +{}ms", TAG, ms);
    }
    Ok(())
}

// Parse and apply one "TAG,TIMESTAMP" line from mobile_event.txt.
pub fn apply_mobile_event_line(line: &str) -> Result<String, String> {
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
    let events_path = crate::data_dir().join("sleep_events.txt");

    let existing = if events_path.exists() {
        std::fs::read_to_string(&events_path).unwrap_or_default()
    } else {
        String::new()
    };

    if existing.lines().any(|l| l.trim() == new_line.as_str()) {
        return Ok(format!("重複スキップ: {}", new_line));
    }

    // Tablet activity while marked as out → insert IN_HOUSE to cancel the out-state.
    // is_out_from_content returns false once IN_HOUSE is in the file, so only one is inserted.
    if event_type == "DEVICE_ON" && is_out_from_content(&existing) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
            let _ = writeln!(f, "{},IN_HOUSE", ts);
        }
    }

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
        let _ = writeln!(f, "{}", new_line);
    } else {
        return Err("書き込み失敗".into());
    }

    if event_type == "DEVICE_ON" {
        let _ = std::fs::write(crate::data_dir().join("device_heartbeat.txt"), format!("{}\n", ts));
    }

    Ok(format!("追加: {}", new_line))
}

pub fn parse_sessions_rust() -> Result<Vec<Session>, String> {
    use chrono::NaiveDateTime;

    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let events_path   = crate::data_dir().join("sleep_events.txt");
    let heartbeat_path = crate::data_dir().join("sleep_heartbeat.txt");
    let min_sleep_secs = crate::THRESHOLD_SECS.load(Ordering::Relaxed) as i64;

    let ts_to_epoch = |s: &str| -> Option<i64> {
        let ndt = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
        Some(ndt.and_utc().timestamp())
    };

    let epoch_to_ts = |ep: i64| -> String {
        use chrono::DateTime;
        let dt = DateTime::from_timestamp(ep, 0).map(|d| d.naive_utc());
        dt.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
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
    if !events_path.exists() {
        eprintln!("{} parse_sessions #{}: cache MISS — no file", TAG, n);
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?;
    let kb = raw.len() as f64 / 1024.0;

    let t0 = std::time::Instant::now();

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
    let event_count = evs.len();

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

    let n_ev = evs.len();
    for i in 0..n_ev {
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
        if ty == "OUT_END" || ty == "IN_HOUSE" { is_out = false; continue; }

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

    let ms = t0.elapsed().as_millis();
    eprintln!("{} parse_sessions #{}: cache MISS — {} events → {} sessions ({:.1}KB)  (+{}ms)",
        TAG, n, event_count, sessions.len(), kb, ms);

    Ok(sessions)
}

#[tauri::command]
pub fn get_sessions() -> Result<Vec<Session>, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let events = crate::data_dir().join("sleep_events.txt");
    let current_mtime = events.metadata()
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH);

    let mut cache = SESSION_CACHE.lock().unwrap();
    if let Some(c) = cache.as_ref() {
        if c.mtime == current_mtime {
            eprintln!("{} parse_sessions #{}: cache HIT", TAG, n);
            return Ok(c.sessions.clone());
        }
    }

    let sessions = parse_sessions_rust()?;
    *cache = Some(SessionCache { sessions: sessions.clone(), mtime: current_mtime });
    Ok(sessions)
}

#[tauri::command]
pub fn add_session(start: String, end: String) -> Result<(), String> {
    eprintln!("{} add_session: {} → {}", TAG, start, end);
    let path = crate::data_dir().join("sleep_events.txt");
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
pub fn delete_session(start: String, end: String) -> Result<(), String> {
    eprintln!("{} delete_session: {} → {}", TAG, start, end);
    let path = crate::data_dir().join("sleep_events.txt");
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

#[tauri::command]
pub fn get_events_content() -> Result<String, String> {
    let path = crate::data_dir().join("sleep_events.txt");
    if !path.exists() { return Ok(String::new()); }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_events(content: String) -> Result<(), String> {
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    sort_events_file(&path)
}

#[tauri::command]
pub fn clear_all_data() -> Result<(), String> {
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, "").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_csv(sessions: Vec<Session>) -> String {
    let mut out = String::from("就寝時刻,起床時刻,睡眠時間(時間),種別\n");
    for s in &sessions {
        out.push_str(&format!("{},{},{:.4},{}\n", s.start, s.end, s.duration_hours, s.session_type));
    }
    out
}

#[tauri::command]
pub fn write_csv_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
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
    eprintln!("{} import_csv: {} sessions added", TAG, added);
    Ok(added)
}
