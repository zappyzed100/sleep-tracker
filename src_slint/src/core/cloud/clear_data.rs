//! clear_data.rs — クラウド全削除・圧縮後の正データ直接反映
//!
//! 役割 : Drive上のバックアップファイル・eventsシートの全消去、およびそれに続けて
//!        ローカルの正の状態を直接push（マージなし）する2つの経路
//!        （全データ削除→リセット反映、データ圧縮→圧縮後の内容を反映）を担当する。
//!
//! 依存 : super::TAG, super::generation::save_local_generation,
//!        super::backup_drive::{backup_to_drive_forced, backup_manual_to_drive_forced},
//!        crate::core::config::load_config_inner

use super::TAG;
use super::generation::save_local_generation;
use super::backup_drive::{backup_to_drive_forced, backup_manual_to_drive_forced};
use crate::core::config::load_config_inner;

// Drive上のバックアップファイル（sleep_events_backup.txt/sleep_manual_backup.txt）と
// eventsシートの行を全消去する。ローカルファイルの削除は呼び出し側が別途行う。
pub fn clear_cloud_data() -> Result<(), String> {
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    // confirm=yes はGAS側の誤爆防止ガード。省略すると"error: clear_all requires
    // confirm=yes"が返るだけで実際の削除は行われない。
    let url = format!("{}?secret={}&action=clear_all&confirm=yes", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client()?
        .post(&url)
        .header("Content-Length", "0")
        .body("")
        .send()
        .map_err(|e| format!("送信失敗: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let body = resp.text().unwrap_or_default();
    let body = body.trim();
    if body.starts_with("error:") {
        eprintln!("{} ERROR clear_cloud_data: rejected by server: {}", TAG, body);
        return Err(body.to_string());
    }
    // action=clear_all は新しい世代番号を返す（GAS側でLockServiceにより排他的に
    // 払い出される）。ここで確定させ、以後のpushをこの端末が「最新」として行える
    // ようにする。
    if let Ok(new_gen) = body.parse::<u64>() {
        save_local_generation(new_gen);
        eprintln!("{} clear_cloud_data: done (generation={})", TAG, new_gen);
    } else {
        eprintln!("{} clear_cloud_data: done", TAG);
    }
    Ok(())
}

// クラウド全削除。action=clear_allだけでは信頼性に難があり、特にsleep_manual.txt
// 側のバックアップが消えないケースを確認している。そのため削除後にローカル
// （events::clear_all_dataで空になっている）の内容を直接pushして確実に上書きする
// （push_authoritative_content_to_driveと同じ「マージせず直接反映」パターン）。
pub fn clear_cloud_data_and_push_reset() -> Result<(), String> {
    clear_cloud_data()?;
    let events_content = std::fs::read_to_string(crate::data_dir().join("sleep_events.txt")).unwrap_or_default();
    let manual_content = std::fs::read_to_string(crate::data_dir().join("sleep_manual.txt")).unwrap_or_default();
    // clear_all直後はほぼ空の内容をpushするため、GAS側の縮小ガードをforceで回避する。
    let events_msg = backup_to_drive_forced(&events_content);
    let manual_msg = backup_manual_to_drive_forced(&manual_content);
    eprintln!("{} clear_cloud_data_and_push_reset: events={} manual={}", TAG, events_msg, manual_msg);
    Ok(())
}

// events::compact_data() で作り直したsleep_events.txtを「新しい正の状態」として
// クラウドにも反映する。通常のsync（Drive→localマージ→push）だと、Driveに残っている
// 圧縮前の内容が通常のunionマージで復活し、圧縮結果が台無しになってしまう。
// そのためここでは先にclear_all（Driveのバックアップファイル・スプレッドシートの
// events行を全消去＋世代番号を新しく進める）してから、圧縮後の内容をマージなしで
// 直接pushする。これにより次回同期時は世代が一致し、この内容がそのまま正として扱われる。
pub fn push_authoritative_content_to_drive(events_content: &str) -> Result<(), String> {
    clear_cloud_data()?;
    // 圧縮・復元は既存の内容と大きさが大きく異なりうるため、GAS側の縮小ガードをforceで回避する。
    let events_msg = backup_to_drive_forced(events_content);
    let manual_content = std::fs::read_to_string(crate::data_dir().join("sleep_manual.txt")).unwrap_or_default();
    let manual_msg = backup_manual_to_drive_forced(&manual_content);
    eprintln!("{} push_authoritative_content_to_drive: events={} manual={}", TAG, events_msg, manual_msg);
    Ok(())
}
