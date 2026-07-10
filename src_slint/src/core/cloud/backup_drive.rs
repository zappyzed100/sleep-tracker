//! backup_drive.rs — sleep_events.txt/sleep_manual.txtのDriveへのpush
//!
//! 役割 : GAS側は検証NG時（HTML混入・縮小ガード等）でもHTTP 200で"error: ..."を
//!        返すため、ステータスだけでなく本文も確認する必要がある。expected_hash
//!        （pull直後の内容のSHA-256）を渡すことで、pull〜push間に別端末が割り込んで
//!        書き込んだ場合、GAS側が拒否してくれる楽観的並行性制御を提供する
//!        （詳細はworker/appsscript.gsの「G. 内容ハッシュ」参照）。
//!
//! 依存 : super::TAG, crate::core::config::load_config_inner
//! 公開 : `backup_to_drive`, `backup_to_drive_forced`（cloud.rsから再公開）,
//!        `backup_to_drive_checked`, `backup_manual_to_drive`, `backup_manual_to_drive_forced`,
//!        `backup_manual_to_drive_checked`, `sha256_hex`（同cloud配下から使用）

use super::TAG;
use crate::core::config::load_config_inner;

// pull直後の内容のSHA-256（16進文字列）。楽観的並行性制御のexpected_hashに使う
// （GAS側でも同じアルゴリズムで計算して比較する。worker/appsscript.gsのcomputeHash_参照）。
pub(super) fn sha256_hex(content: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, content.as_bytes());
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn backup_to_drive(content: &str) -> String {
    backup_to_drive_ex(content, false, None)
}

// 「データを圧縮」「クラウドも含めて全データ削除」のように意図的に大きく
// 縮小した内容をpushする経路専用。GAS側の縮小ガードをforce=1で回避する。
pub fn backup_to_drive_forced(content: &str) -> String {
    backup_to_drive_ex(content, true, None)
}

// pull直後に取得した内容のハッシュをexpected_hashとして渡すことで、pull〜push間に
// 別端末（や手動操作）が割り込んで書き込んだ場合、GAS側が拒否してくれる
// （楽観的並行性制御。詳細はworker/appsscript.gsの「G. 内容ハッシュ」参照）。
// 拒否された場合はbody.trim().starts_with("error:")のため呼び出し側の既存の
// エラーハンドリングにそのまま乗る（"Drive拒否: error: conflict: ..."のような形）。
pub(super) fn backup_to_drive_checked(content: &str, expected_hash: &str) -> String {
    backup_to_drive_ex(content, false, Some(expected_hash))
}

fn backup_to_drive_ex(content: &str, force: bool, expected_hash: Option<&str>) -> String {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Driveスキップ(未設定)".into(),
    };

    let force_param = if force { "&force=1".to_string() } else { String::new() };
    let hash_param = expected_hash.map(|h| format!("&expected_hash={h}")).unwrap_or_default();
    let url = format!("{}?secret={}&action=backup{}{}", base_url.trim_end_matches('/'), secret, force_param, hash_param);
    let kb = content.len() as f64 / 1024.0;
    let resp = match crate::http_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} ERROR backup_to_drive: {}", TAG, e);
            return format!("Drive送信失敗: {}", e);
        }
    };

    let ms = t0.elapsed().as_millis();
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        eprintln!("{} ERROR backup_to_drive: HTTP {}  (+{}ms)", TAG, status, ms);
        return format!("Drive HTTP {}", status);
    }
    let body = resp.text().unwrap_or_default();
    if body.trim().starts_with("error:") {
        eprintln!("{} ERROR backup_to_drive: rejected by server: {}  (+{}ms)", TAG, body.trim(), ms);
        return format!("Drive拒否: {}", body.trim());
    }
    eprintln!("{} backup_to_drive: {:.1}KB sent  (+{}ms)", TAG, kb, ms);
    "Drive バックアップ完了".into()
}

pub(super) fn backup_manual_to_drive(content: &str) -> String {
    backup_manual_to_drive_ex(content, false, None)
}

// backup_to_drive_forced と同様、意図的な縮小pushでGAS側の縮小ガードを回避する。
pub(super) fn backup_manual_to_drive_forced(content: &str) -> String {
    backup_manual_to_drive_ex(content, true, None)
}

// backup_to_drive_checked と同様の楽観的並行性制御（詳細はそちらのコメント参照）。
pub(super) fn backup_manual_to_drive_checked(content: &str, expected_hash: &str) -> String {
    backup_manual_to_drive_ex(content, false, Some(expected_hash))
}

fn backup_manual_to_drive_ex(content: &str, force: bool, expected_hash: Option<&str>) -> String {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Manual Driveスキップ(未設定)".into(),
    };
    let force_param = if force { "&force=1".to_string() } else { String::new() };
    let hash_param = expected_hash.map(|h| format!("&expected_hash={h}")).unwrap_or_default();
    let url = format!("{}?secret={}&action=backup_manual{}{}", base_url.trim_end_matches('/'), secret, force_param, hash_param);
    let kb = content.len() as f64 / 1024.0;
    let resp = match crate::http_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => { eprintln!("{} ERROR backup_manual_to_drive: {}", TAG, e); return format!("Manual Drive送信失敗: {}", e); }
    };
    let ms = t0.elapsed().as_millis();
    if !resp.status().is_success() {
        return format!("Manual Drive HTTP {}", resp.status().as_u16());
    }
    let body = resp.text().unwrap_or_default();
    if body.trim().starts_with("error:") {
        eprintln!("{} ERROR backup_manual_to_drive: rejected by server: {}  (+{}ms)", TAG, body.trim(), ms);
        return format!("Manual Drive拒否: {}", body.trim());
    }
    eprintln!("{} backup_manual_to_drive: {:.1}KB sent  (+{}ms)", TAG, kb, ms);
    "Manual Drive バックアップ完了".into()
}
