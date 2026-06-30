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

use std::sync::atomic::{AtomicU64, Ordering};

use crate::events::{
    SESSION_CACHE, SessionCache, parse_sessions_rust,
    apply_mobile_event_line, sort_events_file,
};
use crate::config::load_config_inner;

const TAG: &str = "[cloud]";

static CONSECUTIVE_ERRORS: AtomicU64 = AtomicU64::new(0);

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

#[tauri::command]
pub fn sync_gist() -> Result<String, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} sync_gist #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    // 1. Pull mobile events from Google Sheets
    let pull_msg = pull_mobile_events_inner();

    // 2. Always sort+dedup the file (removes duplicate IDLE_START lines caused
    //    by monitor oscillation when threshold < WAKE_SECS)
    let events_path = crate::data_dir().join("sleep_events.txt");
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

    let ms = t0.elapsed().as_millis();
    eprintln!("{} sync_gist #{}: done  (+{}ms)", TAG, n, ms);
    Ok(format!("同期完了 — モバイル: {} / {}", pull_msg, drive_msg))
}

pub fn ensure_events_from_drive() {
    let path = crate::data_dir().join("sleep_events.txt");
    if path.exists() { return; }

    eprintln!("{} ensure_events_from_drive: sleep_events.txt missing — fetching from Drive", TAG);
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return,
    };
    let client = match crate::gist_client() { Ok(c) => c, Err(_) => return };
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let t0 = std::time::Instant::now();
    let resp = match client.get(&url).send() {
        Ok(r) if r.status().is_success() => r,
        _ => return,
    };
    let content = match resp.text() {
        Ok(t) if !t.trim().is_empty() => t,
        _ => return,
    };
    let kb = content.len() as f64 / 1024.0;
    let _ = std::fs::write(&path, content);
    let ms = t0.elapsed().as_millis();
    eprintln!("{} ensure_events_from_drive: {:.1}KB restored  (+{}ms)", TAG, kb, ms);
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
    // Write locally so parse_sessions_rust() can read it
    let kb = content.len() as f64 / 1024.0;
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
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
    let url = format!("{}?secret={}&tag=SCREEN_ON&ts={}", base_url.trim_end_matches('/'), secret, ts);
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
