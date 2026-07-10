//! session_ops.rs — 手動セッションの追加・削除（CRUD）
//!
//! 役割 : sleep_manual.txtへの手動睡眠セッション追加、既存セッション（自動検知・
//!        手動どちらも）のソフト削除を担当する。削除はDrive同期を生き残らせるため
//!        マーカー追記方式（MANUAL_DELETED / SESSION_DELETED）を使う。
//!
//! 依存 : super::{TAG, SESSION_CACHE}, super::parsing::{get_sessions, sort_manual_file, sort_events_file},
//!        crate::core::cloud

use std::fs::OpenOptions;
use std::io::Write;

use super::{TAG, SESSION_CACHE};
use super::parsing::{get_sessions, sort_manual_file, sort_events_file};

// 手動追加する睡眠時間が既存のセッションと重なっていないか確認する。
// タイムスタンプは "YYYY-MM-DD HH:MM:SS" 固定長のため文字列比較がそのまま
// 時刻比較として使える。
fn overlaps_existing(start: &str, end: &str) -> Option<(String, String)> {
    let sessions = get_sessions().unwrap_or_default();
    sessions.into_iter()
        .find(|s| start < s.end.as_str() && end > s.start.as_str())
        .map(|s| (s.start, s.end))
}

pub fn add_session(start: String, end: String) -> Result<(), String> {
    if let Some((os, oe)) = overlaps_existing(&start, &end) {
        return Err(format!("既存の睡眠記録（{} → {}）と重なっています", os, oe));
    }
    eprintln!("{} add_session: {} → {}", TAG, start, end);
    let path = crate::data_dir().join("sleep_manual.txt");
    let line = format!("{},{}\n", start, end);
    let mut f = OpenOptions::new().create(true).append(true).open(&path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    *SESSION_CACHE.lock().unwrap() = None;
    let p = path.clone();
    std::thread::spawn(move || { crate::core::cloud::auto_backup_manual(&p); });
    Ok(())
}

pub fn delete_session(start: String, _end: String) -> Result<(), String> {
    eprintln!("{} delete_session: {}", TAG, start);

    // Check if this is a manual session (start exists in sleep_manual.txt as non-deleted entry).
    let manual_path = crate::data_dir().join("sleep_manual.txt");
    if manual_path.exists() {
        let content = std::fs::read_to_string(&manual_path).map_err(|e| e.to_string())?;
        let is_manual = content.lines().any(|l| {
            if let Some(c) = l.find(',') { &l[..c] == start.as_str() && &l[c+1..] != "MANUAL_DELETED" }
            else { false }
        });
        if is_manual {
            // Soft-delete: append MANUAL_DELETED marker so deletion survives Drive sync.
            let marker = format!("{},MANUAL_DELETED\n", start);
            let mut f = OpenOptions::new().create(true).append(true).open(&manual_path)
                .map_err(|e| e.to_string())?;
            f.write_all(marker.as_bytes()).map_err(|e| e.to_string())?;
            eprintln!("{} delete_session: MANUAL_DELETED appended to sleep_manual.txt", TAG);
            sort_manual_file(&manual_path)?;
            *SESSION_CACHE.lock().unwrap() = None;
            let p = manual_path.clone();
            std::thread::spawn(move || { crate::core::cloud::auto_backup_manual(&p); });
            return Ok(());
        }
    }

    // Auto-detected session: soft-delete via SESSION_DELETED marker so the deletion
    // survives sync (the original events remain in the file but are filtered at parse time).
    let events_path = crate::data_dir().join("sleep_events.txt");
    if !events_path.exists() {
        return Err("sleep_events.txt not found".to_string());
    }
    let marker = format!("{},SESSION_DELETED\n", start);
    let mut f = OpenOptions::new().create(true).append(true).open(&events_path)
        .map_err(|e| e.to_string())?;
    f.write_all(marker.as_bytes()).map_err(|e| e.to_string())?;
    sort_events_file(&events_path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    let ep = events_path.clone();
    std::thread::spawn(move || { crate::core::cloud::auto_backup_after_event(&ep); });
    Ok(())
}
