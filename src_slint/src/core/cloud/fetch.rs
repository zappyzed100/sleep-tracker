//! fetch.rs — モバイルイベント・Drive内容の取得
//!
//! 役割 : Google Apps Script経由でモバイルデバイスのイベント（LEAVE_HOME等）を
//!        取得してsleep_events.txtに反映するpull_mobile_events_inner、および
//!        Drive上のsleep_events.txt/sleep_manual.txtのバックアップ内容取得
//!        （クライアント側検証込み）、接続テストを担当する。
//!
//! 依存 : super::{TAG, CONSECUTIVE_ERRORS}, super::generation::{fetch_cloud_generation, merge_or_adopt},
//!        super::validation::{looks_like_events_content, looks_like_html_or_js},
//!        crate::core::config::load_config_inner, crate::core::events

use std::sync::atomic::{AtomicU64, Ordering};

use super::{TAG, CONSECUTIVE_ERRORS};
use super::generation::{fetch_cloud_generation, merge_or_adopt};
use super::validation::{looks_like_events_content, looks_like_html_or_js};
use crate::core::config::load_config_inner;
use crate::core::events::{SESSION_CACHE, SessionCache, parse_sessions_rust, apply_mobile_event_line, sort_events_file};

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

    let client = match crate::http_client() {
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

// Download raw sleep_events.txt content from Drive. Returns None on error / empty / unauthorized.
// GAS側（worker/appsscript.gs）と同じ検証をクライアント側でも独立に行う。
// GAS自身が"error: ..."を返してくれるケース（保存済み内容が壊れていると
// GAS自身が判断した場合）はそれで弾けるが、Googleの認証リダイレクト等
// GASのスクリプトロジックを経由せずに割り込まれるケース（実際に発生した、
// ログインページのHTMLがそのままsleep_events_backup.txtに混入した事故）は
// GAS側の検証をすり抜けるため、クライアント側でも中身を見て判断する必要がある。
pub(super) fn fetch_drive_events(base_url: &str, secret: &str) -> Option<String> {
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    let text = resp.text().ok()?;
    let t = text.trim();
    if t.is_empty() || t == "Unauthorized" || t.starts_with("not found") { return None; }
    if t.starts_with("error:") {
        eprintln!("{} fetch_drive_events: ERROR server-side backup rejected: {}", TAG, t);
        return None;
    }
    if !looks_like_events_content(t) {
        eprintln!("{} fetch_drive_events: ERROR content failed client-side validation (HTML/JS混入またはイベント形式不一致) — discarding", TAG);
        return None;
    }
    Some(text)
}

// Download sleep_manual.txt content from Drive.
// sleep_manual.txtは自由形式のため、HTML/JS混入チェックのみ行う（GAS側と同じ基準）。
pub(super) fn fetch_drive_manual(base_url: &str, secret: &str) -> Option<String> {
    let url = format!("{}?secret={}&action=restore_manual", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    let text = resp.text().ok()?;
    let t = text.trim();
    if t.is_empty() || t == "Unauthorized" || t.starts_with("not found") { return None; }
    if t.starts_with("error:") {
        eprintln!("{} fetch_drive_manual: ERROR server-side backup rejected: {}", TAG, t);
        return None;
    }
    if looks_like_html_or_js(t) {
        eprintln!("{} fetch_drive_manual: ERROR content looks like HTML/JS — discarding", TAG);
        return None;
    }
    Some(text)
}

pub fn fetch_from_cloud() -> Result<Vec<crate::core::events::Session>, String> {
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
    let client = crate::http_client()?;
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
    let cloud_gen = fetch_cloud_generation(&base_url, &secret);
    merge_or_adopt(&path, &content, cloud_gen);
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

pub fn test_mobile_connection(mobile_url: String, mobile_secret: String) -> Result<String, String> {
    if mobile_url.is_empty() || mobile_secret.is_empty() {
        return Err("URL とシークレットを入力してください".to_string());
    }
    let url = format!("{}?secret={}&action=health", mobile_url.trim_end_matches('/'), mobile_secret);
    let resp = crate::http_client()?
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
