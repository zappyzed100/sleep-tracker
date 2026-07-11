//! settings_ui.rs — 設定画面（Settings.tsx 相当）のUI連携（エントリポイント）
//!
//! 役割 : 起動設定・睡眠判定時間・目標起床時刻・クラウド連携・データ管理の
//!        読み込み/保存/操作を実装する各サブモジュール（general/export/
//!        backup_restore/data_management）の公開関数を束ねて再公開する。
//!        共通の確認待ちフラグ・メッセージ種別・完了時刻フォーマットもここで
//!        定義し、`super::`経由で各サブモジュールから使う。
//!
//! 依存 : crate::{MainWindow, BackupEntryVM}, config, platform, cloud, events
//! 公開 : `load_into_window`, `save`, `test_connection`, `toggle_startup`,
//!        `create_shortcut`, `export_csv`, `clear_all_data`, `clear_all_data_and_cloud`,
//!        `compact_data`, `clear_backups`, `sync_now`, `backup`, `restore`,
//!        `open_backup_list`, `close_backup_list`, `restore_from_backup`,
//!        `restore_via_external_picker`, `clear_stale_confirmations`

use crate::MainWindow;
use std::sync::atomic::{AtomicBool, Ordering};

// 全データ削除・復元の誤操作防止用（2回クリックで実行）。設定画面はウィンドウ1つのみなので
// プロセスグローバルなフラグで十分。
pub(super) static CLEAR_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
pub(super) static CLEAR_CLOUD_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
pub(super) static COMPACT_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
pub(super) static CLEAR_BACKUPS_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
pub(super) static RESTORE_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);

// メッセージの種別。SettingsNote側でこの文字列を見て色を変える
// （成功=緑・警告=黄・失敗=赤・それ以外=青）。
pub(super) const KIND_SUCCESS: &str = "success";
pub(super) const KIND_WARN: &str = "warn";
pub(super) const KIND_ERROR: &str = "error";
pub(super) const KIND_INFO: &str = "info";

// 完了メッセージに付ける「今の時刻」（HH:MM:SS）。処理の所要時間ではなく、
// ボタンを押した操作が完了した時刻を表示する。
pub(super) fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

// 「もう一度クリックすると...」の2回クリック確認は、放置すると (a) 確認待ちの
// メッセージが画面に残り続ける (b) ずっと後になって同じボタンを押した時、本人は
// 「1回目のつもり」でも実際は確認済み扱いになり即実行されてしまう、という2つの
// 問題があった。他の操作（別のボタン・設定保存など）を行うたびに呼び出すことで、
// まだ確認待ちのままの破壊的操作をキャンセルし、そのメッセージも消す。
// `keep`には呼び出し元自身のタグを渡す（自分の確認待ち状態はこの後の判定に使うため
// ここでは触らない）。無関係な操作からは""を渡してすべて解除する。
pub fn clear_stale_confirmations(window: &MainWindow, keep: &str) {
    if keep != "restore" && RESTORE_CONFIRM_PENDING.swap(false, Ordering::SeqCst) {
        window.set_restore_message("".into());
    }
    if keep != "clear" && CLEAR_CONFIRM_PENDING.swap(false, Ordering::SeqCst) {
        window.set_clear_message("".into());
    }
    if keep != "clear_cloud" && CLEAR_CLOUD_CONFIRM_PENDING.swap(false, Ordering::SeqCst) {
        window.set_clear_cloud_message("".into());
    }
    if keep != "compact" && COMPACT_CONFIRM_PENDING.swap(false, Ordering::SeqCst) {
        window.set_compact_message("".into());
    }
    if keep != "backups_clear" && CLEAR_BACKUPS_CONFIRM_PENDING.swap(false, Ordering::SeqCst) {
        window.set_backups_clear_message("".into());
    }
}

mod general;
mod export;
mod backup_restore;
mod data_management;

pub use general::{load_into_window, save, toggle_startup, create_shortcut, test_connection, sync_now};
pub use export::export_csv;
pub use backup_restore::{backup, restore, open_backup_list, close_backup_list, restore_from_backup, restore_via_external_picker};
pub use data_management::{clear_all_data, clear_all_data_and_cloud, compact_data, clear_backups};
