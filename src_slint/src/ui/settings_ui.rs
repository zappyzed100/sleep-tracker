//! settings_ui.rs — 設定画面（Settings.tsx 相当）のUI連携
//!
//! 役割 : 起動設定・睡眠判定時間・目標起床時刻・クラウド連携・データ管理の
//!        読み込み/保存/操作をまとめる。CSVエクスポートはrfdのネイティブ
//!        保存ダイアログを使用する。ファイルダイアログを伴う操作（エクスポート・
//!        バックアップ・復元）と全データ削除は別スレッドで実行し、UIスレッドを
//!        ブロックしない。完了メッセージには必ず所要時間（秒）を付ける。
//!
//! 依存 : crate::{MainWindow}, config, platform, cloud, events
//! 公開 : `load_into_window`, `save`, `test_connection`, `toggle_startup`,
//!        `create_shortcut`, `export_csv`, `clear_all_data`, `sync_now`, `backup`, `restore`

use crate::core::{cloud, config, events};
use crate::platform::windows as platform;
use crate::MainWindow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

// 全データ削除・復元の誤操作防止用（2回クリックで実行）。設定画面はウィンドウ1つのみなので
// プロセスグローバルなフラグで十分。
static CLEAR_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
static RESTORE_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);

fn fmt_secs(t0: Instant) -> String {
    format!("{:.2}秒", t0.elapsed().as_secs_f64())
}

pub fn load_into_window(window: &MainWindow) {
    let cfg = config::load_config_inner();
    window.set_startup_enabled(platform::get_startup_enabled());
    window.set_idle_threshold_minutes(cfg.idle_threshold_minutes.unwrap_or(60) as i32);
    if let Some(t) = cfg.target_wake_time.as_deref() {
        let parts: Vec<&str> = t.splitn(2, ':').collect();
        if parts.len() == 2 {
            window.set_target_wake_enabled(true);
            window.set_target_wake_h(parts[0].parse().unwrap_or(7));
            window.set_target_wake_m(parts[1].parse().unwrap_or(0));
        }
    }
    window.set_mobile_url(cfg.mobile_url.unwrap_or_default().into());
    window.set_mobile_secret(cfg.mobile_secret.unwrap_or_default().into());
}

fn current_target_wake(window: &MainWindow) -> Option<String> {
    if window.get_target_wake_enabled() {
        Some(format!("{:02}:{:02}", window.get_target_wake_h(), window.get_target_wake_m()))
    } else {
        None
    }
}

pub fn save(window: &MainWindow) {
    let t0 = Instant::now();
    let idle = window.get_idle_threshold_minutes() as u32;
    let url = window.get_mobile_url().to_string();
    let secret = window.get_mobile_secret().to_string();
    let target = current_target_wake(window);
    match config::save_config(idle, url, secret, target, None) {
        Ok(()) => window.set_save_message(format!("✓ 保存しました ({})", fmt_secs(t0)).into()),
        Err(e) => window.set_save_message(format!("保存失敗: {} ({})", e, fmt_secs(t0)).into()),
    }
}

pub fn toggle_startup(window: &MainWindow, enable: bool) {
    let t0 = Instant::now();
    match platform::set_startup(enable) {
        Ok(()) => {
            let msg = if enable { "✓ 自動起動をONにしました" } else { "✓ 自動起動をOFFにしました" };
            window.set_shortcut_message(format!("{} ({})", msg, fmt_secs(t0)).into());
        }
        Err(e) => {
            window.set_shortcut_message(format!("スタートアップ設定失敗: {} ({})", e, fmt_secs(t0)).into());
            window.set_startup_enabled(!enable);
        }
    }
}

pub fn create_shortcut(window: &MainWindow) {
    let t0 = Instant::now();
    match platform::create_desktop_shortcut() {
        Ok(()) => window.set_shortcut_message(format!("デスクトップにショートカットを作成しました ({})", fmt_secs(t0)).into()),
        Err(e) => window.set_shortcut_message(format!("作成失敗: {} ({})", e, fmt_secs(t0)).into()),
    }
}

// 別スレッドでテスト接続し、完了後にUIスレッドで結果を反映する。
// test-in-progress は呼び出し側（Slint）で即座にtrueへ設定済み。完了後に必ずfalseへ戻す
// （成功・失敗問わず）ことで、ボタンの「テスト中…」表示が固まったままにならないようにする。
pub fn test_connection(weak: slint::Weak<MainWindow>, url: String, secret: String) {
    let t0 = Instant::now();
    std::thread::spawn(move || {
        let result = cloud::test_mobile_connection(url, secret);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(msg) => { w.set_connection_ok(true); w.set_connection_status(format!("{} ({})", msg, fmt_secs(t0)).into()); }
                    Err(e) => { w.set_connection_ok(false); w.set_connection_status(format!("{} ({})", e, fmt_secs(t0)).into()); }
                }
                w.set_test_in_progress(false);
            }
        });
    });
}

// 別スレッドでsync_gistを実行し、完了後にUIスレッドで再読み込みする。
// sync-in-progress も同様に、成功・失敗どちらの経路でも必ずfalseへ戻す。
pub fn sync_now(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let t0 = Instant::now();
    std::thread::spawn(move || {
        let msg = cloud::sync_gist();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                crate::ui::home::refresh_all(&w, &state);
                match msg {
                    Ok(m) => w.set_sync_message(format!("{} ({})", m, fmt_secs(t0)).into()),
                    Err(e) => w.set_sync_message(format!("同期失敗: {} ({})", e, fmt_secs(t0)).into()),
                }
                w.set_sync_in_progress(false);
            }
        });
    });
}

// rfdのファイルダイアログはブロッキングAPIのため、UIスレッドを止めないよう別スレッドで
// 実行する。export-in-progressは呼び出し側（Slint）で即座にtrueへ設定済み。
// rfdクレートはAndroidバックエンドを持たないため、CSVエクスポートは今はデスクトップのみ。
// Android版はファイル共有(Intent)経由の実装が別途必要（今後の課題）。
#[cfg(not(target_os = "android"))]
pub fn export_csv(weak: slint::Weak<MainWindow>) {
    std::thread::spawn(move || {
        let t0 = Instant::now();
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
                    None => {} // ダイアログをキャンセルした場合はメッセージなし
                    Some(Ok(())) => w.set_data_message(format!("CSVエクスポート完了 ({})", fmt_secs(t0)).into()),
                    Some(Err(e)) => w.set_data_message(format!("エクスポート失敗: {} ({})", e, fmt_secs(t0)).into()),
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
    let t0 = Instant::now();
    let Some(w) = weak.upgrade() else { return };
    let Some(dir) = crate::android_external_dir() else {
        w.set_data_message("外部ストレージが利用できません".into());
        w.set_export_in_progress(false);
        return;
    };
    let sessions = events::get_sessions().unwrap_or_default();
    let csv = events::export_csv(&sessions);
    let path = dir.join("sleep_sessions.csv");
    match events::write_csv_file(path.to_string_lossy().to_string(), csv) {
        Ok(()) => w.set_data_message(format!("CSVエクスポート完了 → Android/data/com.sleeptracker.app/files/{} ({})", path.file_name().unwrap().to_string_lossy(), fmt_secs(t0)).into()),
        Err(e) => w.set_data_message(format!("エクスポート失敗: {} ({})", e, fmt_secs(t0)).into()),
    }
    w.set_export_in_progress(false);
}

// バックアップ/リストアもrfd（ネイティブファイルダイアログ）を使うためデスクトップのみ。
#[cfg(not(target_os = "android"))]
pub fn backup(weak: slint::Weak<MainWindow>) {
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let content = events::get_events_content();
        let result = match content {
            Ok(content) => {
                let default_name = format!("sleep_backup_{}.txt", chrono::Local::now().format("%Y-%m-%d"));
                let path = rfd::FileDialog::new()
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
                    None => {} // ダイアログをキャンセルした場合はメッセージなし
                    Some(Ok(path)) => w.set_data_message(format!("バックアップを保存しました → {} ({})", path, fmt_secs(t0)).into()),
                    Some(Err(e)) => w.set_data_message(format!("バックアップ失敗: {} ({})", e, fmt_secs(t0)).into()),
                }
                w.set_backup_in_progress(false);
            }
        });
    });
}

#[cfg(target_os = "android")]
pub fn backup(weak: slint::Weak<MainWindow>) {
    let t0 = Instant::now();
    let Some(w) = weak.upgrade() else { return };
    let Some(dir) = crate::android_external_dir() else {
        w.set_data_message("外部ストレージが利用できません".into());
        w.set_backup_in_progress(false);
        return;
    };
    let content = match events::get_events_content() {
        Ok(c) => c,
        Err(e) => {
            w.set_data_message(format!("バックアップ失敗: {} ({})", e, fmt_secs(t0)).into());
            w.set_backup_in_progress(false);
            return;
        }
    };
    let name = format!("sleep_backup_{}.txt", chrono::Local::now().format("%Y-%m-%d"));
    let path = dir.join(&name);
    match events::write_csv_file(path.to_string_lossy().to_string(), content) {
        Ok(()) => w.set_data_message(format!("バックアップ完了 → Android/data/com.sleeptracker.app/files/{} ({})", name, fmt_secs(t0)).into()),
        Err(e) => w.set_data_message(format!("バックアップ失敗: {} ({})", e, fmt_secs(t0)).into()),
    }
    w.set_backup_in_progress(false);
}

// 誤操作防止のため2回クリックで実行（全データ削除と同様のパターン）。1回目の確認表示は
// 一瞬で終わるため別スレッド化せず、ファイル選択・復元本体（2回目）のみ別スレッドにする。
#[cfg(not(target_os = "android"))]
pub fn restore(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    if !RESTORE_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_data_message("もう一度クリックするとバックアップファイルから復元します（現在のデータは上書きされます）".into());
        window.set_restore_in_progress(false);
        return;
    }
    RESTORE_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let path = rfd::FileDialog::new().add_filter("テキスト", &["txt"]).pick_file();
        let result = path.map(|path| {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("読み込み失敗: {}", e))
                .and_then(|content| events::restore_events(content))
        });
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    None => {} // ファイル選択をキャンセルした場合はメッセージなし
                    Some(Ok(())) => {
                        w.set_data_message(format!("バックアップから復元しました ({})", fmt_secs(t0)).into());
                        crate::ui::home::refresh_all(&w, &state);
                    }
                    Some(Err(e)) => w.set_data_message(format!("復元失敗: {} ({})", e, fmt_secs(t0)).into()),
                }
                w.set_restore_in_progress(false);
            }
        });
    });
}

// ファイルピッカーが使えないため、固定ファイル名（restore.txt）を外部ストレージ領域に
// ファイルマネージャーで置いてもらう方式。誤操作防止のため2回クリックで実行。
#[cfg(target_os = "android")]
pub fn restore(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    let Some(dir) = crate::android_external_dir() else {
        window.set_data_message("外部ストレージが利用できません".into());
        window.set_restore_in_progress(false);
        return;
    };
    let path = dir.join("restore.txt");
    if !RESTORE_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_data_message(
            "Android/data/com.sleeptracker.app/files/restore.txt にバックアップファイルを配置してから、もう一度クリックしてください".into()
        );
        window.set_restore_in_progress(false);
        return;
    }
    RESTORE_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    let t0 = Instant::now();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            window.set_data_message(format!("restore.txt の読み込み失敗: {} ({})", e, fmt_secs(t0)).into());
            window.set_restore_in_progress(false);
            return;
        }
    };
    match events::restore_events(content) {
        Ok(()) => {
            window.set_data_message(format!("バックアップから復元しました ({})", fmt_secs(t0)).into());
            crate::ui::home::refresh_all(&window, &state);
        }
        Err(e) => window.set_data_message(format!("復元失敗: {} ({})", e, fmt_secs(t0)).into()),
    }
    window.set_restore_in_progress(false);
}

pub fn clear_all_data(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    let Some(window) = weak.upgrade() else { return };
    if !CLEAR_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_data_message("もう一度クリックすると全データを削除します".into());
        window.set_clear_in_progress(false);
        return;
    }
    CLEAR_CONFIRM_PENDING.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let result = events::clear_all_data();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(()) => {
                        w.set_data_message(format!("全データを削除しました ({})", fmt_secs(t0)).into());
                        crate::ui::home::refresh_all(&w, &state);
                    }
                    Err(e) => w.set_data_message(format!("削除失敗: {} ({})", e, fmt_secs(t0)).into()),
                }
                w.set_clear_in_progress(false);
            }
        });
    });
}
