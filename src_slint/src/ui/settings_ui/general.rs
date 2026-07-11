//! general.rs — 起動設定・睡眠判定時間・目標起床時刻・クラウド連携の読み込み/保存
//!
//! 役割 : 設定画面を開いた時の初期値読み込み、保存ボタン、PC起動時自動起動・
//!        デスクトップショートカット、クラウド接続テスト、手動同期ボタンを担当する。
//!
//! 依存 : super::{clear_stale_confirmations, now_hms, KIND_*},
//!        crate::core::{cloud, config, events}, crate::platform::windows

use super::{clear_stale_confirmations, now_hms, KIND_ERROR, KIND_SUCCESS, KIND_WARN};
use crate::core::{cloud, config};
use crate::platform::windows as platform;
use crate::MainWindow;

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
    window.set_night_type_boundary_hour(
        cfg.night_type_boundary_hour.unwrap_or(config::NIGHT_TYPE_BOUNDARY_HOUR_DEFAULT) as i32
    );
    window.set_min_screen_on_minutes(
        cfg.min_screen_on_minutes.unwrap_or(config::MIN_SCREEN_ON_MINUTES_DEFAULT) as i32
    );
    window.set_sync_paused(cloud::is_sync_paused());
}

fn current_target_wake(window: &MainWindow) -> Option<String> {
    if window.get_target_wake_enabled() {
        Some(format!("{:02}:{:02}", window.get_target_wake_h(), window.get_target_wake_m()))
    } else {
        None
    }
}

pub fn save(window: &MainWindow) {
    clear_stale_confirmations(window, "");
    let idle = window.get_idle_threshold_minutes() as u32;
    let url = window.get_mobile_url().to_string();
    let secret = window.get_mobile_secret().to_string();
    let target = current_target_wake(window);
    let night_boundary = Some(window.get_night_type_boundary_hour() as f64);
    let min_screen_on_minutes = window.get_min_screen_on_minutes() as u32;
    match config::save_config(idle, url, secret, target, None, night_boundary, min_screen_on_minutes) {
        Ok(()) => {
            window.set_save_message(format!("✓ 保存しました ({})", now_hms()).into());
            window.set_save_kind(KIND_SUCCESS.into());
        }
        Err(e) => {
            window.set_save_message(format!("保存失敗: {} ({})", e, now_hms()).into());
            window.set_save_kind(KIND_ERROR.into());
        }
    }
}

pub fn toggle_startup(window: &MainWindow, enable: bool) {
    clear_stale_confirmations(window, "");
    match platform::set_startup(enable) {
        Ok(()) => {
            let msg = if enable { "✓ 自動起動をONにしました" } else { "✓ 自動起動をOFFにしました" };
            window.set_shortcut_message(format!("{} ({})", msg, now_hms()).into());
            window.set_shortcut_kind(KIND_SUCCESS.into());
        }
        Err(e) => {
            window.set_shortcut_message(format!("スタートアップ設定失敗: {} ({})", e, now_hms()).into());
            window.set_shortcut_kind(KIND_ERROR.into());
            window.set_startup_enabled(!enable);
        }
    }
}

pub fn create_shortcut(window: &MainWindow) {
    clear_stale_confirmations(window, "");
    match platform::create_desktop_shortcut() {
        Ok(true) => {
            window.set_shortcut_message(format!("✓ デスクトップにショートカットを作成しました ({})", now_hms()).into());
            window.set_shortcut_kind(KIND_SUCCESS.into());
        }
        Ok(false) => {
            window.set_shortcut_message(format!("既にショートカットが作成されています（上書きしました） ({})", now_hms()).into());
            window.set_shortcut_kind(KIND_WARN.into());
        }
        Err(e) => {
            window.set_shortcut_message(format!("作成失敗: {} ({})", e, now_hms()).into());
            window.set_shortcut_kind(KIND_ERROR.into());
        }
    }
}

// 別スレッドでテスト接続し、完了後にUIスレッドで結果を反映する。
// test-in-progress は呼び出し側（Slint）で即座にtrueへ設定済み。完了後に必ずfalseへ戻す
// （成功・失敗問わず）ことで、ボタンの「テスト中…」表示が固まったままにならないようにする。
pub fn test_connection(weak: slint::Weak<MainWindow>, url: String, secret: String) {
    if let Some(w) = weak.upgrade() { clear_stale_confirmations(&w, ""); }
    std::thread::spawn(move || {
        let result = cloud::test_mobile_connection(url, secret);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                match result {
                    Ok(msg) => {
                        w.set_connection_ok(true);
                        w.set_connection_status(format!("{} ({})", msg, now_hms()).into());
                    }
                    Err(e) => {
                        w.set_connection_ok(false);
                        w.set_connection_status(format!("{} ({})", e, now_hms()).into());
                    }
                }
                w.set_test_in_progress(false);
            }
        });
    });
}

// 別スレッドでsync_gistを実行し、完了後にUIスレッドで再読み込みする。
// sync-in-progress も同様に、成功・失敗どちらの経路でも必ずfalseへ戻す。
pub fn sync_now(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    if let Some(w) = weak.upgrade() { clear_stale_confirmations(&w, ""); }
    std::thread::spawn(move || {
        let msg = cloud::sync_gist();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                crate::ui::home::refresh_all(&w, &state);
                // 同期成功時の詳細（モバイル/Driveの内訳など）はログにだけ残し、
                // UI表示は「同期完了」と完了時刻だけのシンプルな表示にする。
                match msg {
                    Ok(_) => {
                        w.set_sync_message(format!("✓ 同期完了 ({})", now_hms()).into());
                        w.set_sync_kind(KIND_SUCCESS.into());
                    }
                    Err(e) => {
                        w.set_sync_message(format!("同期失敗: {} ({})", e, now_hms()).into());
                        w.set_sync_kind(KIND_ERROR.into());
                    }
                }
                w.set_sync_in_progress(false);
            }
        });
    });
}
