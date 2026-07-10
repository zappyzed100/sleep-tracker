//! cloud.rs — モバイルイベント取得・Drive バックアップ・クラウド同期（エントリポイント）
//!
//! 役割 : Google Apps Script 経由でモバイルデバイスのイベントを取得し
//!        sleep_events.txt に書き込む。実装を委譲する各サブモジュール
//!        （generation/validation/backup_drive/fetch/sync/clear_data）の公開関数を
//!        束ねて再公開する。内部ファイルへの直接importは禁止、ここ経由でのみ使う。
//!        Tauri版 src-tauri/src/cloud.rs の移植。
//!        `#[tauri::command] async fn` + `spawn_blocking` は同期関数に変更し、
//!        UIスレッドをブロックしないための非同期呼び出しは呼び出し側（main.rs）の
//!        std::thread::spawn に任せる。
//!        全体リセット系操作（全データ削除・データ圧縮）の伝播は、GAS側で
//!        LockServiceにより排他的に払い出す「世代番号」（worker/appsscript.gs参照）
//!        でガードする（詳細はgeneration.rs参照）。世代番号だけでは検知できない
//!        通常のイベント追記どうしの競合（pull〜push間に別端末が割り込んで上書きし、
//!        マージされずに消えるロスト・アップデート）は、pushのたびにpull直後の
//!        内容のSHA-256を送るハッシュベースの楽観的並行性制御で別途検知する
//!        （backup_drive.rs、worker/appsscript.gsの「G. 内容ハッシュ」参照）。
//!
//! 依存 : crate::data_dir, crate::http_client, crate::core::config, crate::core::events
//! 公開 : `pull_mobile_events_inner`, `fetch_from_cloud`,
//!        `sync_gist`, `ensure_events_from_drive`, `test_mobile_connection`,
//!        `clear_cloud_data`, `clear_cloud_data_and_push_reset`, `push_authoritative_content_to_drive`,
//!        `is_sync_paused`, `set_sync_paused`, `auto_backup_after_event`, `auto_backup_manual`,
//!        `sync_mobile_inner`, `backup_to_drive`, `backup_to_drive_forced`

use std::sync::atomic::{AtomicBool, AtomicU64};

const TAG: &str = "[cloud]";

static CONSECUTIVE_ERRORS: AtomicU64 = AtomicU64::new(0);
// Prevents concurrent sync_mobile_inner calls (startup vs manual button press).
static SYNC_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// 自動同期の一時停止フラグ（data_dir/sync_paused ファイルの有無で永続化する）。
// 「同期を停止するボタン」用。手動の「今すぐ同期」ボタン・接続テスト・
// クラウド全削除などの明示的な操作は、このフラグの影響を受けない
// （ユーザーが明示的に押した操作は常に実行されるべきため）。
// 起動時の同期・定期同期・PC側のIDLE_START/RESUMEイベントpushのような
// 「自動で走る」経路だけがこのフラグを見る。
fn sync_paused_flag_path() -> std::path::PathBuf {
    crate::data_dir().join("sync_paused")
}

pub fn is_sync_paused() -> bool {
    sync_paused_flag_path().exists()
}

pub fn set_sync_paused(paused: bool) -> Result<(), String> {
    let path = sync_paused_flag_path();
    if paused {
        std::fs::write(&path, "").map_err(|e| e.to_string())?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    eprintln!("{} set_sync_paused: {}", TAG, paused);
    Ok(())
}

mod generation;
mod validation;
mod backup_drive;
mod fetch;
mod sync;
mod clear_data;
#[cfg(test)]
mod cloud_tests;

pub use fetch::{pull_mobile_events_inner, fetch_from_cloud, test_mobile_connection};
pub use sync::{sync_gist, ensure_events_from_drive, auto_backup_after_event, auto_backup_manual, sync_mobile_inner};
pub use clear_data::{clear_cloud_data, clear_cloud_data_and_push_reset, push_authoritative_content_to_drive};
pub use backup_drive::{backup_to_drive, backup_to_drive_forced};
