//! android_bg.rs — Androidフォアグラウンド中の定期同期
//!
//! 役割 : Tauri版のvisibilitychange同期・30分ごとの定期同期に相当する処理を、
//!        プロセス生存中に限り純Rustで実現する。
//!        Tauri版のDriveSignalWorker（アプリ終了後もWorkManagerで15分ごとに動く
//!        真のバックグラウンド同期）は、Kotlinで書かれたWorkerクラスが必須で
//!        cargo-apk（純Rust構成）の範囲外のため、今回は対象外とする
//!        （アプリを開いている間の同期のみをカバーする）。
//!        日次ローカル自動バックアップ（events::maybe_auto_backup）も同様に
//!        フォアグラウンド中のみの判定になる（アプリを開いた時に前回から
//!        24時間経過していれば取る）。
//!        起動時のDEVICE_ON記録は廃止した（睡眠判定には使っておらず、夜中に
//!        何度も確認する使い方だとログが無駄に増えるだけだったため）。
//!        在宅解除はAPP_USAGE_START（実際にアプリを使った証拠）だけに一本化する。
//!
//! 依存 : crate::{events, cloud, home, sync_status}
//! 公開 : `setup(window: &MainWindow, state: &home::SharedState)`,
//!        `Java_com_sleeptracker_app_MainActivity_nativeOnResume`（KotlinのActivity#onResume()から呼ばれるJNIエントリポイント）

use crate::ui::home::{self, SharedState};
use crate::ui::sync_status;
use crate::core::{cloud, events};
use crate::MainWindow;
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

// 起動時同期と5分ごとの定期同期が重複起動しないようにするためだけのガード
// （sync-in-progressは他の同期要因とも共有する参照カウントのため、これとは別に持つ）。
static RUNNING: AtomicBool = AtomicBool::new(false);

// KotlinのActivity#onResume()から呼ばれるJNI関数(nativeOnResume)が同期をキックできる
// よう、setup()で受け取ったweak/stateをここに保持しておく。
static HANDLE: OnceLock<(slint::Weak<MainWindow>, SharedState)> = OnceLock::new();

pub fn setup(window: &MainWindow, state: &SharedState) {
    let _ = HANDLE.set((window.as_weak(), state.clone()));

    // 起動時: 前回から24時間経っていればローカルバックアップを取る
    events::maybe_auto_backup(&crate::data_dir());

    // 起動直後に1回、即座にバックグラウンド同期する。Slintのリピートタイマーは
    // 最初の発火まで1周期分待つ仕様のため、これが無いと起動から5分間は
    // 同期されず、同期アイコンも動かないままになる。
    run_sync(window.as_weak(), state.clone());

    // 5分ごとにバックグラウンド同期（アプリを開きっぱなしで放置している間の保険）
    let weak = window.as_weak();
    let s = state.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, SYNC_INTERVAL, move || {
        run_sync(weak.clone(), s.clone());
    });
    // Timerをリークして保持する（run()のスコープを抜けても動き続けるように）。
    std::mem::forget(timer);
}

// Kotlin側 MainActivity.onResume() から呼ばれるJNIエントリポイント。
// コールドスタート・タスク切り替えからの復帰・画面ロック解除からの復帰・
// 権限設定画面からの帰還など、「人間がアプリの操作を再開した」あらゆる経路で
// Androidは必ずActivity#onResume()を呼ぶため、ここ一箇所に同期をフックすれば
// 全経路をカバーできる（詳細はplatform/README.md参照）。
// JNIEnv/jclassは使わないため生ポインタで受け、jni crateへの依存を避けている。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sleeptracker_app_MainActivity_nativeOnResume(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    if let Some((weak, state)) = HANDLE.get() {
        run_sync(weak.clone(), state.clone());
    }
}

// バックグラウンド同期を1回実行する。同期中は同期アイコン(sync-in-progress)を
// 手動同期ボタンと同じ見た目で回転させ、完了後は設定タブの同期メッセージも更新する。
// 「同期を停止」中はこの自動経路をスキップする（手動の「今すぐ同期」ボタンは
// sync_gist()経由の別経路なので、停止中でも明示的に押せば同期できる）。
fn run_sync(weak: slint::Weak<MainWindow>, state: SharedState) {
    if cloud::is_sync_paused() { return; }
    if RUNNING.swap(true, Ordering::SeqCst) { return; }
    sync_status::begin(&weak);
    std::thread::spawn(move || {
        let sessions = cloud::sync_mobile_inner();
        eprintln!("[app] android sync: {} sessions", sessions.len());
        events::maybe_auto_backup(&crate::data_dir());
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        let weak2 = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak2.upgrade() { home::refresh_all(&w, &state); }
        });
        RUNNING.store(false, Ordering::SeqCst);
        sync_status::end(&weak, Some((format!("✓ 同期完了 ({})", now), "success")));
    });
}
