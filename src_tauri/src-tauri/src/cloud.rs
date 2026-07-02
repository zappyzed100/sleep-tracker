//! cloud.rs — モバイルイベント取得・Drive バックアップ・Gist 同期
//!
//! 役割 : Google Apps Script 経由でモバイルデバイスのイベントを取得し
//!        sleep_events.txt に書き込む。Drive へのバックアップと
//!        SCREEN_ON 通知の送信も担当する。
//!
//! 依存 : crate::data_dir, crate::gist_client, config::load_config_inner,
//!        events::apply_mobile_event_line, events::sort_events_file,
//!        events::SESSION_CACHE, events::SessionCache, events::parse_sessions_rust
//! 公開 : `pull_mobile_events_inner`, `fetch_from_cloud`, `send_screen_on`,
//!        `sync_gist`, `ensure_events_from_drive`, `test_mobile_connection`

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::events::{
    SESSION_CACHE, SessionCache, parse_sessions_rust,
    apply_mobile_event_line, sort_events_file, sort_manual_file,
    EVENTS_FILE_LOCK,
};
use crate::config::load_config_inner;

const TAG: &str = "[cloud]";

static CONSECUTIVE_ERRORS: AtomicU64 = AtomicU64::new(0);
// Prevents concurrent sync_mobile_inner calls (startup vs manual button press).
static SYNC_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn pull_mobile_events_inner() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} pull_mobile_events #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "スキップ (設定なし)".into(),
    };

    let client = match crate::gist_client() {
        Ok(c) => c,
        Err(e) => {
            let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("{} pull_mobile_events #{}: error ({}回連続) {}", TAG, n, ec, e);
            if ec >= 3 {
                eprintln!("{} pull_mobile_events #{}: WARN {}回連続失敗 — ネットワーク障害の可能性", TAG, n, ec);
            }
            return format!("クライアントエラー: {}", e);
        }
    };

    let url = format!("{}?secret={}", base_url.trim_end_matches('/'), secret);
    let resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("{} pull_mobile_events #{}: error ({}回連続) {}", TAG, n, ec, e);
            if ec >= 3 {
                eprintln!("{} pull_mobile_events #{}: WARN {}回連続失敗 — ネットワーク障害の可能性", TAG, n, ec);
            }
            return format!("取得失敗: {}", e);
        }
    };
    if !resp.status().is_success() {
        let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        let status = resp.status().as_u16();
        eprintln!("{} pull_mobile_events #{}: error ({}回連続) HTTP {}", TAG, n, ec, status);
        return format!("HTTP {}", status);
    }
    let content = match resp.text() {
        Ok(t) => t.trim().to_string(),
        Err(e) => return format!("レスポンス読み取り失敗: {}", e),
    };

    if content.is_empty() || content == "Unauthorized" {
        if content == "Unauthorized" { return "認証失敗（シークレットを確認）".into(); }
        return "モバイルイベントなし".into();
    }

    let kb = content.len() as f64 / 1024.0;
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
        let events_path = crate::data_dir().join("sleep_events.txt");
        let _ = sort_events_file(&events_path);
        *SESSION_CACHE.lock().unwrap() = None;
    }

    CONSECUTIVE_ERRORS.store(0, Ordering::Relaxed);
    let ms = t0.elapsed().as_millis();

    if msgs.is_empty() {
        eprintln!("{} pull_mobile_events #{}: {:.1}KB received, 0 events processed  (+{}ms)", TAG, n, kb, ms);
        return "モバイルイベントなし".into();
    }

    eprintln!("{} pull_mobile_events #{}: {:.1}KB received, {} events processed  (+{}ms)", TAG, n, kb, new_events, ms);
    format!("{} 件処理: {}", msgs.len(), msgs.join(" / "))
}

pub fn backup_to_drive(content: &str) -> String {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Driveスキップ(未設定)".into(),
    };

    let url = format!("{}?secret={}&action=backup", base_url.trim_end_matches('/'), secret);
    let kb = content.len() as f64 / 1024.0;
    let resp = match crate::gist_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} ERROR backup_to_drive: {}", TAG, e);
            return format!("Drive送信失敗: {}", e);
        }
    };

    let ms = t0.elapsed().as_millis();
    if resp.status().is_success() {
        eprintln!("{} backup_to_drive: {:.1}KB sent  (+{}ms)", TAG, kb, ms);
        "Drive バックアップ完了".into()
    } else {
        let status = resp.status().as_u16();
        eprintln!("{} ERROR backup_to_drive: HTTP {}  (+{}ms)", TAG, status, ms);
        format!("Drive HTTP {}", status)
    }
}

// Download raw sleep_events.txt content from Drive. Returns None on error / empty / unauthorized.
fn fetch_drive_events(base_url: &str, secret: &str) -> Option<String> {
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let resp = crate::gist_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    let text = resp.text().ok()?;
    let t = text.trim();
    if t.is_empty() || t == "Unauthorized" || t.starts_with("not found") { return None; }
    Some(text)
}

// Download sleep_manual.txt content from Drive.
fn fetch_drive_manual(base_url: &str, secret: &str) -> Option<String> {
    let url = format!("{}?secret={}&action=restore_manual", base_url.trim_end_matches('/'), secret);
    let resp = crate::gist_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    let text = resp.text().ok()?;
    let t = text.trim();
    if t.is_empty() || t == "Unauthorized" || t.starts_with("not found") { return None; }
    Some(text)
}

fn backup_manual_to_drive(content: &str) -> String {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Manual Driveスキップ(未設定)".into(),
    };
    let url = format!("{}?secret={}&action=backup_manual", base_url.trim_end_matches('/'), secret);
    let kb = content.len() as f64 / 1024.0;
    let resp = match crate::gist_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => { eprintln!("{} ERROR backup_manual_to_drive: {}", TAG, e); return format!("Manual Drive送信失敗: {}", e); }
    };
    let ms = t0.elapsed().as_millis();
    if resp.status().is_success() {
        eprintln!("{} backup_manual_to_drive: {:.1}KB sent  (+{}ms)", TAG, kb, ms);
        "Manual Drive バックアップ完了".into()
    } else {
        format!("Manual Drive HTTP {}", resp.status().as_u16())
    }
}

// Merge drive_content lines into the local file (sort by timestamp, dedup).
// Returns true if the local file was updated (new lines added from Drive).
fn merge_into_local(path: &std::path::Path, drive_content: &str) -> bool {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    // EVENTS_FILE_LOCK を取得して、sort_events_file / apply_mobile_event_line との
    // 競合によるデータ消失を防ぐ（merge中の並行append/sortをブロックする）
    let _lock = EVENTS_FILE_LOCK.lock().unwrap();

    let local_raw = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else { String::new() };
    // Strip UTF-8 BOM (U+FEFF) that PowerShell/Windows tools sometimes prepend.
    let local = local_raw.trim_start_matches('\u{FEFF}');
    let drive_content = drive_content.trim_start_matches('\u{FEFF}');

    // 処理済みのローカル行（BOM除去・trim・空行フィルタ済み）を先に作成
    let local_processed: Vec<String> = local.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let local_n = local_processed.len();
    let drive_n = drive_content.lines().filter(|l| !l.trim().is_empty()).count();
    eprintln!("{} merge_into_local #{}: local={} lines  drive={} lines", TAG, n, local_n, drive_n);

    let drive_processed: Vec<String> = drive_content.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}').to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let mut all: Vec<String> = local_processed.iter()
        .chain(drive_processed.iter())
        .cloned()
        .collect();
    // Sort by full content so dedup removes ALL duplicates (same-timestamp
    // alternating pairs like IN_HOUSE/STARTUP would survive timestamp-only sort).
    all.sort();
    all.dedup();
    // Re-sort by timestamp for chronological order.
    all.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));

    // Write if content changed (new lines added OR duplicates removed from local)
    let local_normalized = local_processed.join("\n") + "\n";
    let merged = all.join("\n") + "\n";
    if merged == local_normalized && all.len() == local_n {
        eprintln!("{} merge_into_local #{}: no change ({}  lines)", TAG, n, all.len());
        return false;
    }

    let delta = all.len() as i64 - local_n as i64;
    eprintln!("{} merge_into_local #{}: {} → {} lines ({:+})", TAG, n, local_n, all.len(), delta);
    if delta > 0 {
        // local_processed（trim済み）と比較して正確に追加行を特定
        let added_lines: Vec<&str> = all.iter()
            .filter(|l| !local_processed.iter().any(|lp| lp == l.as_str()))
            .map(|s| s.as_str()).collect();
        for (i, line) in added_lines.iter().enumerate().take(10) {
            eprintln!("{} merge_into_local #{}:   +[{}] {}", TAG, n, i, line);
        }
        if added_lines.len() > 10 {
            eprintln!("{} merge_into_local #{}: ... ({} more)", TAG, n, added_lines.len() - 10);
        }
    }

    let _ = std::fs::write(path, merged);
    true
}

#[tauri::command]
pub fn sync_gist() -> Result<String, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} sync_gist #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let events_path = crate::data_dir().join("sleep_events.txt");
    let manual_path = crate::data_dir().join("sleep_manual.txt");

    let cfg = load_config_inner();
    let url_secret = if let (Some(u), Some(s)) = (cfg.mobile_url, cfg.mobile_secret) {
        if !u.is_empty() && !s.is_empty() { Some((u, s)) } else { None }
    } else { None };

    // 0. Drive → local merge (sleep_events.txt and sleep_manual.txt)
    if let Some((ref u, ref s)) = url_secret {
        if let Some(drive_content) = fetch_drive_events(u, s) {
            let kb = drive_content.len() as f64 / 1024.0;
            eprintln!("{} sync_gist #{}: {:.1}KB events from Drive", TAG, n, kb);
            if merge_into_local(&events_path, &drive_content) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
        }
        if let Some(drive_manual) = fetch_drive_manual(u, s) {
            let kb = drive_manual.len() as f64 / 1024.0;
            eprintln!("{} sync_gist #{}: {:.1}KB manual from Drive", TAG, n, kb);
            if merge_into_local(&manual_path, &drive_manual) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
            let _ = sort_manual_file(&manual_path);
        }
    }

    // 1. Pull mobile events from Google Sheets
    let pull_msg = pull_mobile_events_inner();

    // 2. Sort+dedup sleep_events.txt
    if events_path.exists() {
        let _ = sort_events_file(&events_path);
    }

    // 3. Read updated sleep_events.txt
    let content = if events_path.exists() {
        std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };

    // 4. Upload both files to Drive (local → Drive)
    let drive_msg = backup_to_drive(&content);
    if let Ok(manual_content) = std::fs::read_to_string(&manual_path) {
        let _ = backup_manual_to_drive(&manual_content);
    }

    let ms = t0.elapsed().as_millis();
    eprintln!("{} sync_gist #{}: done  (+{}ms)", TAG, n, ms);
    Ok(format!("同期完了 — モバイル: {} / {}", pull_msg, drive_msg))
}

pub fn ensure_events_from_drive() {
    let path = crate::data_dir().join("sleep_events.txt");
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => {
            if !path.exists() {
                eprintln!("{} ensure_events_from_drive: cloud not configured and no local file", TAG);
            }
            return;
        }
    };

    eprintln!("{} ensure_events_from_drive: fetching from Drive", TAG);
    let t0 = std::time::Instant::now();
    match fetch_drive_events(&base_url, &secret) {
        Some(drive_content) => {
            let kb = drive_content.len() as f64 / 1024.0;
            merge_into_local(&path, &drive_content);
            eprintln!("{} ensure_events_from_drive: {:.1}KB  (+{}ms)", TAG, kb, t0.elapsed().as_millis());
        }
        None => {
            eprintln!("{} ensure_events_from_drive: Drive unavailable  (+{}ms)", TAG, t0.elapsed().as_millis());
        }
    }
}

// Back up sleep_events.txt to Drive after writes.
// Drive→ローカルマージ → push することで、他デバイスがDriveに書き込んだイベントの上書き消失を防ぐ。
// Spawned in a thread; no-op if not configured.
pub fn auto_backup_after_event(events_path: &std::path::Path) {
    // 1. Driveからマージ（他デバイスのイベントを取り込む）
    let cfg = load_config_inner();
    if let (Some(u), Some(s)) = (cfg.mobile_url.clone(), cfg.mobile_secret.clone()) {
        if !u.is_empty() && !s.is_empty() {
            if let Some(drive_content) = fetch_drive_events(&u, &s) {
                if merge_into_local(events_path, &drive_content) {
                    *SESSION_CACHE.lock().unwrap() = None;
                }
            }
        }
    }
    // 2. マージ済みのローカルファイルをDriveにpush
    if let Ok(content) = std::fs::read_to_string(events_path) {
        let msg = backup_to_drive(&content);
        eprintln!("{} auto_backup: {}", TAG, msg);
    }
}

// Back up sleep_manual.txt to Drive after writes. Spawned in a thread; no-op if not configured.
pub fn auto_backup_manual(manual_path: &std::path::Path) {
    if let Ok(content) = std::fs::read_to_string(manual_path) {
        let msg = backup_manual_to_drive(&content);
        eprintln!("{} auto_backup_manual: {}", TAG, msg);
    }
}

// Core sync logic shared by Android startup, focus events, and the "今すぐ同期" button.
// Merge Drive → local → pull Sheet → sort → upload → parse sessions.
// Returns cached sessions immediately if another sync is already in progress.
pub fn sync_mobile_inner() -> Vec<crate::events::Session> {
    static N: AtomicU64 = AtomicU64::new(0);

    if SYNC_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        eprintln!("{} sync_mobile_inner: already running — returning cache", TAG);
        return SESSION_CACHE.lock().unwrap()
            .as_ref().map(|c| c.sessions.clone()).unwrap_or_default();
    }

    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} sync_mobile_inner #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let events_path = crate::data_dir().join("sleep_events.txt");
    let manual_path = crate::data_dir().join("sleep_manual.txt");

    // 1. Fetch settings (update THRESHOLD_SECS from Drive)
    let _ = crate::config::fetch_settings_from_cloud();

    // 2. Drive → local merge (sleep_events.txt and sleep_manual.txt)
    let cfg = load_config_inner();
    if let (Some(u), Some(s)) = (cfg.mobile_url, cfg.mobile_secret) {
        if !u.is_empty() && !s.is_empty() {
            if let Some(drive_content) = fetch_drive_events(&u, &s) {
                let kb = drive_content.len() as f64 / 1024.0;
                eprintln!("{} sync_mobile_inner #{}: {:.1}KB events from Drive", TAG, n, kb);
                if merge_into_local(&events_path, &drive_content) {
                    *SESSION_CACHE.lock().unwrap() = None;
                }
            }
            if let Some(drive_manual) = fetch_drive_manual(&u, &s) {
                let kb = drive_manual.len() as f64 / 1024.0;
                eprintln!("{} sync_mobile_inner #{}: {:.1}KB manual from Drive", TAG, n, kb);
                if merge_into_local(&manual_path, &drive_manual) {
                    *SESSION_CACHE.lock().unwrap() = None;
                }
                let _ = sort_manual_file(&manual_path);
            }
        }
    }

    // 3. Pull mobile events from Sheet (SCREEN_ON / LEAVE_HOME / ARRIVE_HOME)
    pull_mobile_events_inner();

    // 4. Sort + dedup
    if events_path.exists() {
        let _ = sort_events_file(&events_path);
    }

    // 5. Upload both files to Drive (local → Drive)
    if let Ok(content) = std::fs::read_to_string(&events_path) {
        let drive_msg = backup_to_drive(&content);
        eprintln!("{} sync_mobile_inner #{}: upload events: {}", TAG, n, drive_msg);
    }
    if let Ok(manual_content) = std::fs::read_to_string(&manual_path) {
        let manual_msg = backup_manual_to_drive(&manual_content);
        eprintln!("{} sync_mobile_inner #{}: upload manual: {}", TAG, n, manual_msg);
    }

    // 6. Parse sessions (rebuild cache)
    let sessions = parse_sessions_rust().unwrap_or_default();
    let mtime_events = events_path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    let mtime_manual = manual_path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    let mtime = mtime_events.max(mtime_manual);
    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions: sessions.clone(), mtime });

    let ms = t0.elapsed().as_millis();
    eprintln!("{} sync_mobile_inner #{}: {} sessions  (+{}ms)", TAG, n, sessions.len(), ms);

    SYNC_IN_PROGRESS.store(false, Ordering::SeqCst);
    sessions
}

// Android "今すぐ同期" Tauri command — thin wrapper around sync_mobile_inner.
#[tauri::command]
pub fn sync_mobile() -> Result<Vec<crate::events::Session>, String> {
    Ok(sync_mobile_inner())
}

#[tauri::command]
pub fn fetch_from_cloud() -> Result<Vec<crate::events::Session>, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} fetch_from_cloud #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let client = crate::gist_client()?;
    let resp = client.get(&url).send().map_err(|e| {
        let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!("{} fetch_from_cloud #{}: error ({}回連続) {}", TAG, n, ec, e);
        if ec >= 3 {
            eprintln!("{} fetch_from_cloud #{}: WARN {}回連続失敗 — ネットワーク障害の可能性", TAG, n, ec);
        }
        format!("取得失敗: {}", e)
    })?;
    if !resp.status().is_success() {
        let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        let status = resp.status().as_u16();
        eprintln!("{} fetch_from_cloud #{}: error ({}回連続) HTTP {}", TAG, n, ec, status);
        return Err(format!("HTTP {}", status));
    }
    let content = resp.text().map_err(|e| format!("レスポンス読み取り失敗: {}", e))?;
    if content.trim().is_empty() || content.trim() == "Unauthorized" {
        if content.trim() == "Unauthorized" {
            return Err("認証失敗（シークレットを確認）".into());
        }
        return Ok(vec![]);
    }
    // Driveから取得した内容をローカルにマージ（上書きではなく統合）
    let kb = content.len() as f64 / 1024.0;
    let path = crate::data_dir().join("sleep_events.txt");
    merge_into_local(&path, &content);
    // Invalidate and rebuild cache
    *SESSION_CACHE.lock().unwrap() = None;
    let sessions = parse_sessions_rust()?;
    let mtime = path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions: sessions.clone(), mtime });

    CONSECUTIVE_ERRORS.store(0, Ordering::Relaxed);
    let ms = t0.elapsed().as_millis();
    eprintln!("{} fetch_from_cloud #{}: {:.1}KB received  (+{}ms)", TAG, n, kb, ms);
    Ok(sessions)
}

#[tauri::command]
pub fn send_screen_on() -> Result<String, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    let ts = chrono::Local::now().timestamp_millis();
    let url = format!("{}?secret={}&tag=APP_FOREGROUND&ts={}", base_url.trim_end_matches('/'), secret, ts);
    let client = crate::gist_client()?;
    let resp = client
        .post(&url)
        .header("Content-Length", "0")
        .body("")
        .send()
        .map_err(|e| format!("送信失敗: {}", e))?;
    if resp.status().is_success() {
        eprintln!("{} send_screen_on #{}", TAG, n);
        Ok("SCREEN_ON 送信完了".into())
    } else {
        Err(format!("HTTP {}", resp.status().as_u16()))
    }
}

#[tauri::command]
pub fn test_mobile_connection(mobile_url: String, mobile_secret: String) -> Result<String, String> {
    if mobile_url.is_empty() || mobile_secret.is_empty() {
        return Err("URL とシークレットを入力してください".to_string());
    }
    let url = format!("{}?secret={}&action=health", mobile_url.trim_end_matches('/'), mobile_secret);
    let resp = crate::gist_client()?
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