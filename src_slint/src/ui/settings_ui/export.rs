//! export.rs — CSVエクスポート
//!
//! 役割 : セッション一覧をCSVとして書き出す。PC版はrfdのネイティブ保存ダイアログ、
//!        Android版はアプリ専用の外部ストレージ領域に固定ファイル名で書き出す。
//!
//! 依存 : super::{clear_stale_confirmations, now_hms, KIND_*}, crate::core::events

use super::{clear_stale_confirmations, now_hms, KIND_ERROR, KIND_SUCCESS};
use crate::core::events;
use crate::MainWindow;

// rfdのファイルダイアログはブロッキングAPIのため、UIスレッドを止めないよう別スレッドで
// 実行する。export-in-progressは呼び出し側（Slint）で即座にtrueへ設定済み。
// rfdクレートはAndroidバックエンドを持たないため、CSVエクスポートは今はデスクトップのみ。
// Android版はファイル共有(Intent)経由の実装が別途必要（今後の課題）。
#[cfg(not(target_os = "android"))]
pub fn export_csv(weak: slint::Weak<MainWindow>) {
    if let Some(w) = weak.upgrade() { clear_stale_confirmations(&w, ""); }
    std::thread::spawn(move || {
        let sessions = events::get_sessions().unwrap_or_default();
        let csv = events::export_csv(&sessions);
        let path = rfd::FileDialog::new()
            .set_file_name("sleep_sessions.csv")
            .add_filter("CSV", &["csv"])
            .save_file();
        let result = path.map(|p| events::write_csv_file(p.to_string_lossy().to_string(), csv));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    None => {
                        w.set_export_message("キャンセルしました".into());
                        w.set_export_kind(super::KIND_INFO.into());
                    }
                    Some(Ok(())) => {
                        w.set_export_message(format!("✓ CSVエクスポート完了 ({})", now_hms()).into());
                        w.set_export_kind(KIND_SUCCESS.into());
                    }
                    Some(Err(e)) => {
                        w.set_export_message(format!("エクスポート失敗: {} ({})", e, now_hms()).into());
                        w.set_export_kind(KIND_ERROR.into());
                    }
                }
                w.set_export_in_progress(false);
            }
        });
    });
}

// Android版はrfdが使えないため、アプリ専用の外部ストレージ領域
// （/storage/emulated/0/Android/data/com.sleeptracker.app/files/）に固定ファイル名で書き出す。
// 特別な権限なしにファイルマネージャーから参照・取り出しができる。
#[cfg(target_os = "android")]
pub fn export_csv(weak: slint::Weak<MainWindow>) {
    let Some(w) = weak.upgrade() else { return };
    clear_stale_confirmations(&w, "");
    let Some(dir) = crate::android_external_dir() else {
        w.set_export_message("外部ストレージが利用できません".into());
        w.set_export_kind(KIND_ERROR.into());
        w.set_export_in_progress(false);
        return;
    };
    let sessions = events::get_sessions().unwrap_or_default();
    let csv = events::export_csv(&sessions);
    let path = dir.join("sleep_sessions.csv");
    match events::write_csv_file(path.to_string_lossy().to_string(), csv) {
        Ok(()) => {
            w.set_export_message(format!("✓ CSVエクスポート完了 → Android/data/com.sleeptracker.app/files/{} ({})", path.file_name().unwrap().to_string_lossy(), now_hms()).into());
            w.set_export_kind(KIND_SUCCESS.into());
        }
        Err(e) => {
            w.set_export_message(format!("エクスポート失敗: {} ({})", e, now_hms()).into());
            w.set_export_kind(KIND_ERROR.into());
        }
    }
    w.set_export_in_progress(false);
}
