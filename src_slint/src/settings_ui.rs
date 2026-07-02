//! settings_ui.rs — 設定画面（Settings.tsx 相当）のUI連携
//!
//! 役割 : 起動設定・睡眠判定時間・目標起床時刻・クラウド連携・データ管理の
//!        読み込み/保存/操作をまとめる。CSVエクスポートはrfdのネイティブ
//!        保存ダイアログを使用する。
//!
//! 依存 : crate::{MainWindow}, config, platform, cloud, events
//! 公開 : `load_into_window`, `save`, `test_connection`, `toggle_startup`,
//!        `create_shortcut`, `export_csv`, `clear_all_data`, `sync_now`

use crate::{cloud, config, events, platform, MainWindow};
use std::sync::atomic::{AtomicBool, Ordering};

// 全データ削除の誤操作防止用（2回クリックで実行）。設定画面はウィンドウ1つのみなので
// プロセスグローバルなフラグで十分。
static CLEAR_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);

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
    let idle = window.get_idle_threshold_minutes() as u32;
    let url = window.get_mobile_url().to_string();
    let secret = window.get_mobile_secret().to_string();
    let target = current_target_wake(window);
    match config::save_config(idle, url, secret, target, None) {
        Ok(()) => window.set_settings_message("保存しました".into()),
        Err(e) => window.set_settings_message(format!("保存失敗: {}", e).into()),
    }
}

pub fn toggle_startup(window: &MainWindow, enable: bool) {
    match platform::set_startup(enable) {
        Ok(()) => {}
        Err(e) => {
            window.set_settings_message(format!("スタートアップ設定失敗: {}", e).into());
            window.set_startup_enabled(!enable);
        }
    }
}

pub fn create_shortcut(window: &MainWindow) {
    match platform::create_desktop_shortcut() {
        Ok(()) => window.set_settings_message("デスクトップショートカットを作成しました".into()),
        Err(e) => window.set_settings_message(format!("ショートカット作成失敗: {}", e).into()),
    }
}

// 別スレッドでテスト接続し、完了後にUIスレッドで結果を反映する。
pub fn test_connection(weak: slint::Weak<MainWindow>, url: String, secret: String) {
    std::thread::spawn(move || {
        let result = cloud::test_mobile_connection(url, secret);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(msg) => { w.set_connection_ok(true); w.set_connection_status(msg.into()); }
                    Err(e) => { w.set_connection_ok(false); w.set_connection_status(e.into()); }
                }
            }
        });
    });
}

// 別スレッドでsync_gistを実行し、完了後にUIスレッドで再読み込みする。
pub fn sync_now(weak: slint::Weak<MainWindow>, state: crate::home::SharedState) {
    std::thread::spawn(move || {
        let msg = cloud::sync_gist();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                crate::home::refresh_all(&w, &state);
                match msg {
                    Ok(m) => w.set_settings_message(m.into()),
                    Err(e) => w.set_settings_message(format!("同期失敗: {}", e).into()),
                }
            }
        });
    });
}

// rfdクレートはAndroidバックエンドを持たないため、CSVエクスポートは今はデスクトップのみ。
// Android版はファイル共有(Intent)経由の実装が別途必要（今後の課題）。
#[cfg(not(target_os = "android"))]
pub fn export_csv(window: &MainWindow) {
    let sessions = events::get_sessions().unwrap_or_default();
    let csv = events::export_csv(&sessions);
    let path = rfd::FileDialog::new()
        .set_file_name("sleep_sessions.csv")
        .add_filter("CSV", &["csv"])
        .save_file();
    let Some(path) = path else { return };
    match events::write_csv_file(path.to_string_lossy().to_string(), csv) {
        Ok(()) => window.set_settings_message("CSVエクスポート完了".into()),
        Err(e) => window.set_settings_message(format!("エクスポート失敗: {}", e).into()),
    }
}

#[cfg(target_os = "android")]
pub fn export_csv(window: &MainWindow) {
    window.set_settings_message("CSVエクスポートはAndroid版では未対応です".into());
}

pub fn clear_all_data(window: &MainWindow, state: &crate::home::SharedState) {
    if !CLEAR_CONFIRM_PENDING.swap(true, Ordering::SeqCst) {
        window.set_settings_message("もう一度クリックすると全データを削除します".into());
        return;
    }
    CLEAR_CONFIRM_PENDING.store(false, Ordering::SeqCst);
    match events::clear_all_data() {
        Ok(()) => {
            window.set_settings_message("全データを削除しました".into());
            crate::home::refresh_all(window, state);
        }
        Err(e) => window.set_settings_message(format!("削除失敗: {}", e).into()),
    }
}
