//! sync.rs — Drive⇔ローカルの同期オーケストレーション
//!
//! 役割 : sync_gist（週間同期ボタン・定期同期）、sync_mobile_inner（Android
//!        起動時・フォーカス時・「今すぐ同期」ボタンで共有される中核同期ロジック）、
//!        auto_backup_after_event/auto_backup_manual（イベント発生直後の即時Drive反映、
//!        ハッシュ競合時は即座にリトライする）を担当する。
//!
//! 依存 : super::{TAG, SYNC_IN_PROGRESS}, super::generation::*, super::fetch::*,
//!        super::backup_drive::*, crate::core::{config, events}

use std::sync::atomic::{AtomicU64, Ordering};

use super::{TAG, SYNC_IN_PROGRESS};
use super::generation::{fetch_cloud_generation, merge_or_adopt, generation_unchanged_since};
use super::fetch::{fetch_drive_events, fetch_drive_manual, pull_mobile_events_inner};
use super::backup_drive::{
    sha256_hex, backup_to_drive, backup_to_drive_checked,
    backup_manual_to_drive, backup_manual_to_drive_checked,
};
use crate::core::config::load_config_inner;
use crate::core::events::{SESSION_CACHE, SessionCache, parse_sessions_rust, sort_events_file, sort_manual_file};

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
    let cloud_gen = url_secret.as_ref().and_then(|(u, s)| fetch_cloud_generation(u, s));
    if let Some((ref u, ref s)) = url_secret {
        if let Some(drive_content) = fetch_drive_events(u, s) {
            let kb = drive_content.len() as f64 / 1024.0;
            eprintln!("{} sync_gist #{}: {:.1}KB events from Drive", TAG, n, kb);
            if merge_or_adopt(&events_path, &drive_content, cloud_gen) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
        }
        if let Some(drive_manual) = fetch_drive_manual(u, s) {
            let kb = drive_manual.len() as f64 / 1024.0;
            eprintln!("{} sync_gist #{}: {:.1}KB manual from Drive", TAG, n, kb);
            if merge_or_adopt(&manual_path, &drive_manual, cloud_gen) {
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

    // 3. push直前に世代が変わっていないか再確認する。pull時点より進んでいたら、
    // このマージ結果は既に古くなっている可能性があるためpushを見送る。
    let should_push = url_secret.as_ref()
        .map(|(u, s)| generation_unchanged_since(u, s, cloud_gen))
        .unwrap_or(true);
    let drive_msg = if should_push {
        // Read updated sleep_events.txt and upload both files to Drive (local → Drive)
        let content = if events_path.exists() {
            std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?
        } else {
            String::new()
        };
        let drive_msg = backup_to_drive(&content);
        if let Ok(manual_content) = std::fs::read_to_string(&manual_path) {
            let _ = backup_manual_to_drive(&manual_content);
        }
        drive_msg
    } else {
        "スキップ (クラウドが更新済み)".into()
    };

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
            let cloud_gen = fetch_cloud_generation(&base_url, &secret);
            merge_or_adopt(&path, &drive_content, cloud_gen);
            eprintln!("{} ensure_events_from_drive: {:.1}KB  (+{}ms)", TAG, kb, t0.elapsed().as_millis());
        }
        None => {
            eprintln!("{} ensure_events_from_drive: Drive unavailable  (+{}ms)", TAG, t0.elapsed().as_millis());
        }
    }
}

// Back up sleep_events.txt to Drive after writes.
// Drive→ローカルマージ → push することで、他デバイスがDriveに書き込んだイベントの上書き消失を防ぐ。
// 呼び出し側で std::thread::spawn すること。
//
// pull〜push間のハッシュ競合（他端末が割り込んで書き込んだ）は即座に再試行する。
// 周期同期の間隔を60秒→10分に緩和した際、IDLE_START直後の一発pushが競合で
// 拒否されると次の周期同期（最大10分後）までAndroid側にIDLE_STARTが届かなく
// なる問題が発覚したため追加した（IDLE_RESUME側は別途pullし直すため気づかれ
// にくかった）。ネットワーク断・検証NG等の競合以外の失敗は即時リトライしても
// 改善しないため1回で諦め、次回の周期同期に任せる。
pub fn auto_backup_after_event(events_path: &std::path::Path) {
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        // 1. Driveからマージ（他デバイスのイベントを取り込む）
        let cfg = load_config_inner();
        let url_secret = if let (Some(u), Some(s)) = (cfg.mobile_url.clone(), cfg.mobile_secret.clone()) {
            if !u.is_empty() && !s.is_empty() { Some((u, s)) } else { None }
        } else { None };
        let cloud_gen = url_secret.as_ref().and_then(|(u, s)| fetch_cloud_generation(u, s));
        // pull時点の内容のハッシュを覚えておき、push時にexpected_hashとして送る
        // （pull〜push間に別端末が割り込んで書き込んだ場合、GAS側で拒否させるため。
        // fetch自体に失敗した場合はNoneのままとなり、従来通りチェック無しでpushする）。
        let mut pulled_hash: Option<String> = None;
        if let Some((ref u, ref s)) = url_secret {
            if let Some(drive_content) = fetch_drive_events(u, s) {
                pulled_hash = Some(sha256_hex(&drive_content));
                if merge_or_adopt(events_path, &drive_content, cloud_gen) {
                    *SESSION_CACHE.lock().unwrap() = None;
                }
            }
        }
        // 2. push直前に世代が変わっていないか再確認してからDriveにpush
        let should_push = url_secret.as_ref()
            .map(|(u, s)| generation_unchanged_since(u, s, cloud_gen))
            .unwrap_or(true);
        if !should_push {
            eprintln!("{} auto_backup: skip upload (cloud generation advanced mid-sync)", TAG);
            return;
        }
        let Ok(content) = std::fs::read_to_string(events_path) else { return };
        let msg = match &pulled_hash {
            Some(h) => backup_to_drive_checked(&content, h),
            None => backup_to_drive(&content),
        };
        eprintln!("{} auto_backup #{}: {}", TAG, attempt, msg);
        if !msg.contains("conflict") {
            return;
        }
        if attempt < MAX_ATTEMPTS {
            eprintln!("{} auto_backup: conflict — retrying ({}/{})", TAG, attempt + 1, MAX_ATTEMPTS);
        }
    }
    eprintln!("{} auto_backup: gave up after {} attempts (persistent conflict)", TAG, MAX_ATTEMPTS);
}

// Back up sleep_manual.txt to Drive after writes. 呼び出し側で std::thread::spawn すること。
pub fn auto_backup_manual(manual_path: &std::path::Path) {
    if let Ok(content) = std::fs::read_to_string(manual_path) {
        let msg = backup_manual_to_drive(&content);
        eprintln!("{} auto_backup_manual: {}", TAG, msg);
    }
}

// Core sync logic shared by Android startup, focus events, and the "今すぐ同期" button.
// Merge Drive → local → pull Sheet → sort → upload → parse sessions.
// Returns cached sessions immediately if another sync is already in progress.
pub fn sync_mobile_inner() -> Vec<crate::core::events::Session> {
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
    let _ = crate::core::config::fetch_settings_from_cloud();

    // 3. Pull mobile events from Sheet (LEAVE_HOME / ARRIVE_HOME / APP_USAGE_START / APP_USAGE_END)
    pull_mobile_events_inner();

    // 4. Sort + dedup
    if events_path.exists() {
        let _ = sort_events_file(&events_path);
    }

    // 2・5. Drive → local merge → push。ハッシュ競合（pull〜push間に別端末が割り込んで
    // 書き込んだ）の場合だけ即座に再試行する（詳細はauto_backup_after_event参照）。
    let cfg = load_config_inner();
    let url_secret = if let (Some(u), Some(s)) = (cfg.mobile_url, cfg.mobile_secret) {
        if !u.is_empty() && !s.is_empty() { Some((u, s)) } else { None }
    } else { None };
    const MAX_ATTEMPTS: u32 = 3;
    let mut conflict = false;
    for attempt in 1..=MAX_ATTEMPTS {
        let cloud_gen = url_secret.as_ref().and_then(|(u, s)| fetch_cloud_generation(u, s));
        // pull時点の内容のハッシュを覚えておき、push時にexpected_hashとして送る
        // （fetch自体に失敗した場合はNoneのままとなり、従来通りチェック無しでpushする）。
        let mut events_pulled_hash: Option<String> = None;
        let mut manual_pulled_hash: Option<String> = None;
        if let Some((ref u, ref s)) = url_secret {
            if let Some(drive_content) = fetch_drive_events(u, s) {
                let kb = drive_content.len() as f64 / 1024.0;
                eprintln!("{} sync_mobile_inner #{}: {:.1}KB events from Drive", TAG, n, kb);
                events_pulled_hash = Some(sha256_hex(&drive_content));
                if merge_or_adopt(&events_path, &drive_content, cloud_gen) {
                    *SESSION_CACHE.lock().unwrap() = None;
                }
            }
            if let Some(drive_manual) = fetch_drive_manual(u, s) {
                let kb = drive_manual.len() as f64 / 1024.0;
                eprintln!("{} sync_mobile_inner #{}: {:.1}KB manual from Drive", TAG, n, kb);
                manual_pulled_hash = Some(sha256_hex(&drive_manual));
                if merge_or_adopt(&manual_path, &drive_manual, cloud_gen) {
                    *SESSION_CACHE.lock().unwrap() = None;
                }
                let _ = sort_manual_file(&manual_path);
            }
        }

        // push直前に世代が変わっていないか再確認する。pull時点より進んでいたら、
        // このマージ結果は既に古くなっている可能性があるためpushを見送る。
        let should_push = url_secret.as_ref()
            .map(|(u, s)| generation_unchanged_since(u, s, cloud_gen))
            .unwrap_or(true);
        if !should_push {
            eprintln!("{} sync_mobile_inner #{}: skip upload (cloud generation advanced mid-sync)", TAG, n);
            break;
        }

        conflict = false;
        if let Ok(content) = std::fs::read_to_string(&events_path) {
            let drive_msg = match &events_pulled_hash {
                Some(h) => backup_to_drive_checked(&content, h),
                None => backup_to_drive(&content),
            };
            eprintln!("{} sync_mobile_inner #{}: upload events: {}", TAG, n, drive_msg);
            conflict |= drive_msg.contains("conflict");
        }
        if let Ok(manual_content) = std::fs::read_to_string(&manual_path) {
            let manual_msg = match &manual_pulled_hash {
                Some(h) => backup_manual_to_drive_checked(&manual_content, h),
                None => backup_manual_to_drive(&manual_content),
            };
            eprintln!("{} sync_mobile_inner #{}: upload manual: {}", TAG, n, manual_msg);
            conflict |= manual_msg.contains("conflict");
        }
        if !conflict || attempt == MAX_ATTEMPTS {
            break;
        }
        eprintln!("{} sync_mobile_inner #{}: conflict — retrying ({}/{})", TAG, n, attempt + 1, MAX_ATTEMPTS);
    }
    if conflict {
        eprintln!("{} sync_mobile_inner #{}: gave up after {} attempts (persistent conflict)", TAG, n, MAX_ATTEMPTS);
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
