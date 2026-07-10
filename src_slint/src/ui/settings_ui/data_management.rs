//! data_management.rs — 全データ削除・データ圧縮・バックアップ履歴削除
//!
//! 役割 : ローカル/クラウド全データ削除、sleep_events.txt/sleep_manual.txtの圧縮、
//!        ローカル自動バックアップ履歴の削除を担当する。いずれも破壊的操作のため
//!        2回クリック確認パターンを使う。
//!
//! 依存 : super::{clear_stale_confirmations, now_hms, KIND_*, *_CONFIRM_PENDING},
//!        crate::core::{cloud, events}

use std::sync::atomic::Ordering;

use super::{
    clear_stale_confirmations, now_hms, KIND_ERROR, KIND_SUCCESS, KIND_WARN,
    CLEAR_CONFIRM_PENDING, CLEAR_CLOUD_CONFIRM_PENDING, COMPACT_CONFIRM_PENDING, CLEAR_BACKUPS_CONFIRM_PENDING,
};
use crate::core::{cloud, events};
use crate::MainWindow;

// ローカルの sleep_events.txt / sleep_manual.txt だけを削除する（クラウドは残る）。
pub fn clear_all_data(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    clear_stale_confirmations(&window, "clear");
    if !CLEAR_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_clear_message("もう一度クリックするとローカルの全データを削除します".into());
        window.set_clear_kind(KIND_WARN.into());
        window.set_clear_in_progress(false);
        return;
    }
    CLEAR_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let result = events::clear_all_data();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        w.set_clear_message(format!("✓ ローカルの全データを削除しました ({})", now_hms()).into());
                        w.set_clear_kind(KIND_SUCCESS.into());
                        crate::ui::home::refresh_all(&w, &state);
                    }
                    Err(e) => {
                        w.set_clear_message(format!("削除失敗: {} ({})", e, now_hms()).into());
                        w.set_clear_kind(KIND_ERROR.into());
                    }
                }
                w.set_clear_in_progress(false);
            }
        });
    });
}

// ローカルに加えてクラウド（Driveのバックアップファイル・スプレッドシートのevents行）も
// 削除する。ローカルより一段階重い操作のため、確認フラグはローカル削除とは別に持つ。
pub fn clear_all_data_and_cloud(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    clear_stale_confirmations(&window, "clear_cloud");
    if !CLEAR_CLOUD_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_clear_cloud_message("もう一度クリックするとクラウドも含めて全データを削除します".into());
        window.set_clear_cloud_kind(KIND_WARN.into());
        window.set_clear_cloud_in_progress(false);
        return;
    }
    CLEAR_CLOUD_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let local_result = events::clear_all_data();
        // クラウド削除は直接反映（clear_cloud_data_and_push_reset）を使う。action=clear_all
        // だけでは信頼性に難があるため、削除後にローカル（空になっている）を直接pushして
        // 確実に上書きする。もう一方の端末は世代番号（cloud::fetch_cloud_generation参照）で
        // このリセットを検知し、古いデータを復活させずに揃う。
        let cloud_result = match &local_result {
            Ok(()) => cloud::clear_cloud_data_and_push_reset(),
            Err(e) => Err(e.clone()),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match (local_result, cloud_result) {
                    (Ok(()), Ok(())) => {
                        w.set_clear_cloud_message(format!("✓ ローカル・クラウドとも全データを削除しました ({})", now_hms()).into());
                        w.set_clear_cloud_kind(KIND_SUCCESS.into());
                        crate::ui::home::refresh_all(&w, &state);
                    }
                    (Ok(()), Err(e)) => {
                        w.set_clear_cloud_message(format!("ローカルは削除済み、クラウド削除失敗: {} ({})", e, now_hms()).into());
                        w.set_clear_cloud_kind(KIND_ERROR.into());
                        crate::ui::home::refresh_all(&w, &state);
                    }
                    (Err(e), _) => {
                        w.set_clear_cloud_message(format!("削除失敗: {} ({})", e, now_hms()).into());
                        w.set_clear_cloud_kind(KIND_ERROR.into());
                    }
                }
                w.set_clear_cloud_in_progress(false);
            }
        });
    });
}

// sleep_events.txt/sleep_manual.txtを、実際にセッションとしてパースされる内容だけの
// 最小構成に作り直す（events::compact_data 参照）。ローカルで作り直した後、その内容を
// クラウド（Drive・スプレッドシート）にも直接反映する。破壊的な操作のため2回クリック確認。
pub fn compact_data(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    clear_stale_confirmations(&window, "compact");
    if !COMPACT_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_compact_message("もう一度クリックするとデータを圧縮します".into());
        window.set_compact_kind(KIND_WARN.into());
        window.set_compact_in_progress(false);
        return;
    }
    COMPACT_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let result = events::compact_data();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(content) => {
                        w.set_compact_message(format!("✓ ローカルを圧縮しました。クラウドに反映中… ({})", now_hms()).into());
                        w.set_compact_kind(KIND_SUCCESS.into());
                        crate::ui::home::refresh_all(&w, &state);

                        std::thread::spawn(move || {
                            let cloud_result = cloud::push_authoritative_content_to_drive(&content);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(w) = weak.upgrade() {
                                    match cloud_result {
                                        Ok(()) => {
                                            w.set_compact_message(format!("✓ 圧縮完了（ローカル・クラウドとも） ({})", now_hms()).into());
                                            w.set_compact_kind(KIND_SUCCESS.into());
                                        }
                                        Err(e) => {
                                            w.set_compact_message(format!("ローカルは圧縮済み、クラウド反映失敗: {} ({})", e, now_hms()).into());
                                            w.set_compact_kind(KIND_ERROR.into());
                                        }
                                    }
                                    w.set_compact_in_progress(false);
                                }
                            });
                        });
                    }
                    Err(e) => {
                        w.set_compact_message(format!("圧縮失敗: {} ({})", e, now_hms()).into());
                        w.set_compact_kind(KIND_ERROR.into());
                        w.set_compact_in_progress(false);
                    }
                }
            }
        });
    });
}

// data/backups/（日次ローカル自動バックアップの蓄積分）を全削除する。
// sleep_events.txt/sleep_manual.txt自体やクラウドには触れない、比較的軽い操作。
pub fn clear_backups(weak: slint::Weak<MainWindow>) {
    let Some(window) = weak.upgrade() else { return };
    clear_stale_confirmations(&window, "backups_clear");
    if !CLEAR_BACKUPS_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_backups_clear_message("もう一度クリックするとバックアップ履歴を削除します".into());
        window.set_backups_clear_kind(KIND_WARN.into());
        window.set_backups_clear_in_progress(false);
        return;
    }
    CLEAR_BACKUPS_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let result = events::clear_backups(&crate::backups_base_dir());
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        w.set_backups_clear_message(format!("✓ バックアップ履歴を削除しました ({})", now_hms()).into());
                        w.set_backups_clear_kind(KIND_SUCCESS.into());
                    }
                    Err(e) => {
                        w.set_backups_clear_message(format!("削除失敗: {} ({})", e, now_hms()).into());
                        w.set_backups_clear_kind(KIND_ERROR.into());
                    }
                }
                w.set_backups_clear_in_progress(false);
            }
        });
    });
}
