//! backup_restore.rs — 手動バックアップ・バックアップからの復元
//!
//! 役割 : ファイルダイアログ（PC）/固定パス（Android）へのバックアップ書き出し、
//!        バックアップ一覧モーダル、一覧からの復元、外部ファイルピッカー経由の復元を担当する。
//!
//! 依存 : super::{clear_stale_confirmations, now_hms, KIND_*, RESTORE_CONFIRM_PENDING},
//!        crate::core::events, crate::{BackupEntryVM, MainWindow}

use std::sync::atomic::Ordering;

use super::{clear_stale_confirmations, now_hms, KIND_ERROR, KIND_INFO, KIND_SUCCESS, KIND_WARN, RESTORE_CONFIRM_PENDING};
use crate::core::events;
use crate::{BackupEntryVM, MainWindow};

// バックアップ/リストアもrfd（ネイティブファイルダイアログ）を使うためデスクトップのみ。
#[cfg(not(target_os = "android"))]
pub fn backup(weak: slint::Weak<MainWindow>) {
    if let Some(w) = weak.upgrade() { clear_stale_confirmations(&w, ""); }
    std::thread::spawn(move || {
        let content = events::get_events_content();
        let result = match content {
            Ok(content) => {
                // 自動バックアップと同じフォルダをデフォルトの保存先にする。ここに保存すれば
                // 「バックアップから復元」の一覧にも自動的に出てくる（新しい順ソート込みで）。
                let backups_dir = crate::backups_base_dir().join("backups");
                let _ = std::fs::create_dir_all(&backups_dir);
                let default_name = format!("sleep_backup_{}.txt", chrono::Local::now().format("%Y-%m-%d"));
                let path = rfd::FileDialog::new()
                    .set_directory(&backups_dir)
                    .set_file_name(&default_name)
                    .add_filter("テキスト", &["txt"])
                    .save_file();
                path.map(|p| events::write_csv_file(p.to_string_lossy().to_string(), content).map(|()| p.display().to_string()))
            }
            Err(e) => Some(Err(e)),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    None => {
                        w.set_backup_message("キャンセルしました".into());
                        w.set_backup_kind(KIND_INFO.into());
                    }
                    Some(Ok(path)) => {
                        w.set_backup_message(format!("✓ バックアップを保存しました → {} ({})", path, now_hms()).into());
                        w.set_backup_kind(KIND_SUCCESS.into());
                    }
                    Some(Err(e)) => {
                        w.set_backup_message(format!("バックアップ失敗: {} ({})", e, now_hms()).into());
                        w.set_backup_kind(KIND_ERROR.into());
                    }
                }
                w.set_backup_in_progress(false);
            }
        });
    });
}

#[cfg(target_os = "android")]
pub fn backup(weak: slint::Weak<MainWindow>) {
    let Some(w) = weak.upgrade() else { return };
    clear_stale_confirmations(&w, "");
    let Some(dir) = crate::android_external_dir() else {
        w.set_backup_message("外部ストレージが利用できません".into());
        w.set_backup_kind(KIND_ERROR.into());
        w.set_backup_in_progress(false);
        return;
    };
    let content = match events::get_events_content() {
        Ok(c) => c,
        Err(e) => {
            w.set_backup_message(format!("バックアップ失敗: {} ({})", e, now_hms()).into());
            w.set_backup_kind(KIND_ERROR.into());
            w.set_backup_in_progress(false);
            return;
        }
    };
    // 自動バックアップと同じbackups/フォルダに保存する。「バックアップから復元」の
    // 一覧にも自動的に出てくる（新しい順ソート込みで）。
    let backups_dir = dir.join("backups");
    let _ = std::fs::create_dir_all(&backups_dir);
    let name = format!("sleep_backup_{}.txt", chrono::Local::now().format("%Y-%m-%d"));
    let path = backups_dir.join(&name);
    match events::write_csv_file(path.to_string_lossy().to_string(), content) {
        Ok(()) => {
            w.set_backup_message(format!("✓ バックアップ完了 → Android/data/com.sleeptracker.app/files/backups/{} ({})", name, now_hms()).into());
            w.set_backup_kind(KIND_SUCCESS.into());
        }
        Err(e) => {
            w.set_backup_message(format!("バックアップ失敗: {} ({})", e, now_hms()).into());
            w.set_backup_kind(KIND_ERROR.into());
        }
    }
    w.set_backup_in_progress(false);
}

// 誤操作防止のため2回クリックで実行（全データ削除と同様のパターン）。2回目でOSの
// ファイルピッカーを直接開くのではなく、アプリ内の「バックアップ一覧」を表示する
// （新しい順ソート・デフォルトの場所をアプリ側で保証するため。OSのファイルピッカー
// にはソート順を指定するAPIが無く、Androidはさらに専用フォルダへのナビゲーションを
// OS側で制限しているため、どちらも「新しい順」「デフォルトでそこが開く」を保証できない）。
pub fn restore(weak: slint::Weak<MainWindow>, _state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    clear_stale_confirmations(&window, "restore");
    if !RESTORE_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_restore_message("もう一度クリックするとバックアップ一覧を表示します（現在のデータは上書きされます）".into());
        window.set_restore_kind(KIND_WARN.into());
        window.set_restore_in_progress(false);
        return;
    }
    RESTORE_CONFIRM_PENDING.store(false, Ordering::SeqCst);
    window.set_restore_in_progress(false);
    open_backup_list(&window);
}

// バックアップ一覧（backups/フォルダの中身、新しい順）を読み込んでモーダルを開く。
pub fn open_backup_list(window: &MainWindow) {
    let entries: Vec<BackupEntryVM> = events::list_backups().into_iter()
        .map(|b| BackupEntryVM { path: b.path.into(), label: b.label.into() })
        .collect();
    window.set_backup_list(slint::ModelRc::new(slint::VecModel::from(entries)));
    window.set_show_backup_list(true);
}

pub fn close_backup_list(window: &MainWindow) {
    window.set_show_backup_list(false);
}

// 一覧内の1件をタップして復元する。ファイルの場所は既知（backups/内）なので
// OSのダイアログを介さず直接読み込む。
pub fn restore_from_backup(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState, path: String) {
    let Some(window) = weak.upgrade() else { return };
    clear_stale_confirmations(&window, "");
    window.set_show_backup_list(false);
    let result = std::fs::read_to_string(&path)
        .map_err(|e| format!("読み込み失敗: {}", e))
        .and_then(events::restore_events);
    match result {
        Ok(()) => {
            window.set_restore_message(format!("✓ バックアップから復元しました ({})", now_hms()).into());
            window.set_restore_kind(KIND_SUCCESS.into());
            crate::ui::home::refresh_all(&window, &state);
        }
        Err(e) => {
            window.set_restore_message(format!("復元失敗: {} ({})", e, now_hms()).into());
            window.set_restore_kind(KIND_ERROR.into());
        }
    }
}

// 一覧の「その他のファイルを選択…」— backups/フォルダ以外の任意の場所にある
// ファイルから復元したい場合のための、OSのファイルピッカー経由の代替経路。
#[cfg(not(target_os = "android"))]
pub fn restore_via_external_picker(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    if let Some(w) = weak.upgrade() {
        clear_stale_confirmations(&w, "");
        w.set_show_backup_list(false);
        w.set_restore_in_progress(true);
    }
    std::thread::spawn(move || {
        let path = rfd::FileDialog::new()
            .set_directory(crate::backups_base_dir().join("backups"))
            .add_filter("テキスト", &["txt"])
            .pick_file();
        let result = path.map(|path| {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("読み込み失敗: {}", e))
                .and_then(events::restore_events)
        });
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    None => {
                        w.set_restore_message("キャンセルしました".into());
                        w.set_restore_kind(KIND_INFO.into());
                    }
                    Some(Ok(())) => {
                        w.set_restore_message(format!("✓ バックアップから復元しました ({})", now_hms()).into());
                        w.set_restore_kind(KIND_SUCCESS.into());
                        crate::ui::home::refresh_all(&w, &state);
                    }
                    Some(Err(e)) => {
                        w.set_restore_message(format!("復元失敗: {} ({})", e, now_hms()).into());
                        w.set_restore_kind(KIND_ERROR.into());
                    }
                }
                w.set_restore_in_progress(false);
            }
        });
    });
}

// Kotlin側のACTION_OPEN_DOCUMENTシステムファイルピッカーを起動する
// （platform::android::launch_picker）。完了・キャンセル・失敗の反映は
// JNIコールバック(nativeRestorePicked)側で行われる。
#[cfg(target_os = "android")]
pub fn restore_via_external_picker(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    if let Some(w) = weak.upgrade() {
        clear_stale_confirmations(&w, "");
        w.set_show_backup_list(false);
        w.set_restore_in_progress(true);
    }
    crate::platform::android::launch_picker(weak, state);
}
