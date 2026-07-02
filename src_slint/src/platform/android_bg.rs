//! android_bg.rs — Androidフォアグラウンド中の定期同期・起動時DEVICE_ON記録
//!
//! 役割 : Tauri版のvisibilitychange同期・30分ごとの定期同期に相当する処理を、
//!        プロセス生存中に限り純Rustで実現する。
//!        Tauri版のDriveSignalWorker（アプリ終了後もWorkManagerで15分ごとに動く
//!        真のバックグラウンド同期）は、Kotlinで書かれたWorkerクラスが必須で
//!        cargo-apk（純Rust構成）の範囲外のため、今回は対象外とする
//!        （アプリを開いている間の同期のみをカバーする）。
//!
//! 依存 : crate::{events, cloud, home}
//! 公開 : `setup(window: &MainWindow, state: &home::SharedState)`

use crate::ui::home::{self, SharedState};
use crate::core::{cloud, events};
use crate::MainWindow;
use slint::ComponentHandle;

const SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub fn setup(window: &MainWindow, state: &SharedState) {
    // 起動時: アプリを開いたことを記録（Tauri版のrecord_device_on相当）
    events::record_device_on();

    // 5分ごとにバックグラウンド同期（アプリが存命中のみ）
    let timer = slint::Timer::default();
    let weak = window.as_weak();
    let s = state.clone();
    timer.start(slint::TimerMode::Repeated, SYNC_INTERVAL, move || {
        let weak = weak.clone();
        let s = s.clone();
        std::thread::spawn(move || {
            let sessions = cloud::sync_mobile_inner();
            eprintln!("[app] android periodic sync: {} sessions", sessions.len());
            let weak = weak.clone();
            let s = s.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { home::refresh_all(&w, &s); }
            });
        });
    });
    // Timerをリークして保持する（run()のスコープを抜けても動き続けるように）。
    std::mem::forget(timer);
}
