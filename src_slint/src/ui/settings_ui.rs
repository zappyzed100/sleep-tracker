//! settings_ui.rs — 設定画面（Settings.tsx 相当）のUI連携
//!
//! 役割 : 起動設定・睡眠判定時間・目標起床時刻・クラウド連携・データ管理の
//!        読み込み/保存/操作をまとめる。CSVエクスポートはrfdのネイティブ
//!        保存ダイアログを使用する。ファイルダイアログを伴う操作（エクスポート・
//!        バックアップ・復元）と全データ削除は別スレッドで実行し、UIスレッドを
//!        ブロックしない。完了メッセージには必ずボタンを押した時刻を付け、
//!        成功/警告/失敗を色分けできるよう種別（kind）も一緒に設定する。
//!
//! 依存 : crate::{MainWindow, BackupEntryVM, UsagePackageVM}, config, platform, cloud, events
//! 公開 : `load_into_window`, `save`, `test_connection`, `toggle_startup`,
//!        `create_shortcut`, `export_csv`, `clear_all_data`, `clear_all_data_and_cloud`,
//!        `compact_data`, `clear_backups`, `sync_now`, `backup`, `restore`,
//!        `open_backup_list`, `close_backup_list`, `restore_from_backup`,
//!        `restore_via_external_picker`, `clear_stale_confirmations`,
//!        `load_usage_packages`, `toggle_usage_package`

use crate::core::{cloud, config, events};
use crate::platform::windows as platform;
use crate::{BackupEntryVM, MainWindow, UsagePackageVM};
use std::sync::atomic::{AtomicBool, Ordering};

// 全データ削除・復元の誤操作防止用（2回クリックで実行）。設定画面はウィンドウ1つのみなので
// プロセスグローバルなフラグで十分。
static CLEAR_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
static CLEAR_CLOUD_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
static COMPACT_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
static CLEAR_BACKUPS_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);
static RESTORE_CONFIRM_PENDING: AtomicBool = AtomicBool::new(false);

// メッセージの種別。SettingsNote側でこの文字列を見て色を変える
// （成功=緑・警告=黄・失敗=赤・それ以外=青）。
const KIND_SUCCESS: &str = "success";
const KIND_WARN: &str = "warn";
const KIND_ERROR: &str = "error";
const KIND_INFO: &str = "info";

// 完了メッセージに付ける「今の時刻」（HH:MM:SS）。処理の所要時間ではなく、
// ボタンを押した操作が完了した時刻を表示する。
fn now_hms() -> String {
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
    window.set_sync_paused(cloud::is_sync_paused());
    load_usage_packages(window);
}

// 「睡眠判定に使うアプリ」一覧を読み込んでウィンドウへ反映する。sleep_events.txt経由の
// 通常同期で他端末の検知結果も届くため、起動時だけでなくhome::refresh_all（同期後の
// 再読み込み）からも呼ばれる。
pub fn load_usage_packages(window: &MainWindow) {
    let entries: Vec<UsagePackageVM> = events::list_usage_packages().into_iter()
        .map(|e| UsagePackageVM { package: e.package.into(), label: e.label.into(), allowed: e.allowed })
        .collect();
    window.set_usage_packages(slint::ModelRc::new(slint::VecModel::from(entries)));
}

// 設定画面のトグルから呼ぶ。ON/OFFを反映し、一覧を再読み込みする。
pub fn toggle_usage_package(weak: slint::Weak<MainWindow>, package: String, new_allowed: bool) {
    let Some(window) = weak.upgrade() else { return };
    if let Err(e) = events::set_usage_package_allowed(&package, new_allowed) {
        eprintln!("[app] toggle_usage_package: ERROR {}", e);
    }
    load_usage_packages(&window);
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
    match config::save_config(idle, url, secret, target, None) {
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
                        w.set_export_kind(KIND_INFO.into());
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
// （platform::android_restore::launch_picker）。完了・キャンセル・失敗の反映は
// JNIコールバック(nativeRestorePicked)側で行われる。
#[cfg(target_os = "android")]
pub fn restore_via_external_picker(weak: slint::Weak<MainWindow>, state: crate::ui::home::SharedState) {
    if let Some(w) = weak.upgrade() {
        clear_stale_confirmations(&w, "");
        w.set_show_backup_list(false);
        w.set_restore_in_progress(true);
    }
    crate::platform::android_restore::launch_picker(weak, state);
}

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
        // だけでは信頼性に難があるため、削除後にローカル（HARD_RESETマーカーのみ）を
        // 直接pushして確実に上書きする。これにより、もう一方の端末も次回同期時に
        // HARD_RESETマーカーを検知して古いデータを復活させずに揃う。
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
                            let cloud_result = cloud::push_compacted_to_drive(&content);
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
