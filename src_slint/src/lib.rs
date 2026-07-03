//! lib.rs — sleep_tracker (Rust + Slint) 共有ロジック・エントリポイント
//!
//! 役割 : デスクトップ（src/main.rs）・Android（src/platform/android.rs）の両方から
//!        呼ばれる共通ロジック。Slintウィンドウの起動、共有static
//!        （THRESHOLD_SECS, HTTP_CLIENT）、パスユーティリティ（data_dir, config_path）
//!        を定義する。core/（ビジネスロジック）・platform/（OS固有機能）・
//!        ui/（画面ロジック）を宣言し、起動時の初期化と全コールバックの配線を行う。
//!
//! 公開 : `run`, `THRESHOLD_SECS`, `data_dir`, `config_path`, `http_client`,
//!        `Session`, `init_android_app_dir`（Android専用）

mod core;
mod platform;
mod ui;

use core::{cloud, config, events, prediction};
use ui::{home, settings_ui};

pub use core::Session;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

slint::include_modules!();

// Shared threshold: updated instantly by save_config, read by monitor thread.
pub static THRESHOLD_SECS: AtomicU64 = AtomicU64::new(3600);

static HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

// ── Path utilities ────────────────────────────────────────────────────────────
//
// デスクトップ: exe の場所から上に辿って ui/main.slint をマーカーにsrc_slint/
// 自身を探す。データ・設定ファイルはsrc_slint/配下に自己完結させる
// （以前はTauri版のsrc_tauri/data/を間借りしていたが、src_tauri/を削除すると
// データを見失う脆い作りだったため、src_slint単体で完結するよう変更した）。
// Android: setup()でAndroidのアプリ内部ストレージパスを渡してもらう（Tauri版のAPP_DIR相当）。

#[cfg(target_os = "android")]
static ANDROID_APP_DIR: OnceLock<PathBuf> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn init_android_app_dir(path: PathBuf) {
    let _ = ANDROID_APP_DIR.set(path);
}

// アプリ専用の外部ストレージ領域（/storage/emulated/0/Android/data/<package>/files/）。
// スコープドストレージ配下でも特別な権限なしにファイルマネージャーから参照できるため、
// CSVエクスポート・バックアップ・リストア（rfdが使えないAndroidの代替）に使う。
#[cfg(target_os = "android")]
static ANDROID_EXTERNAL_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn init_android_external_dir(path: Option<PathBuf>) {
    let _ = ANDROID_EXTERNAL_DIR.set(path);
}

#[cfg(target_os = "android")]
pub fn android_external_dir() -> Option<PathBuf> {
    ANDROID_EXTERNAL_DIR.get().cloned().flatten()
}

#[cfg(not(target_os = "android"))]
fn app_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        let mut dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        for _ in 0..8 {
            if dir.join("ui").join("main.slint").exists() { return dir; }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
        std::env::current_dir().unwrap_or_default()
    })
}

#[cfg(not(target_os = "android"))]
pub fn data_dir() -> PathBuf {
    static DATA: OnceLock<PathBuf> = OnceLock::new();
    DATA.get_or_init(|| {
        let dir = app_root().join("data");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }).clone()
}

#[cfg(target_os = "android")]
pub fn data_dir() -> PathBuf {
    ANDROID_APP_DIR.get().expect("init_android_app_dir が呼ばれていません").clone()
}

#[cfg(not(target_os = "android"))]
pub fn config_path() -> PathBuf {
    app_root().join("config.json")
}

#[cfg(target_os = "android")]
pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn http_client() -> Result<&'static reqwest::blocking::Client, String> {
    if HTTP_CLIENT.get().is_none() {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let _ = HTTP_CLIENT.set(client);
    }
    Ok(HTTP_CLIENT.get().unwrap())
}

pub fn run() {
    // 多重起動防止（デスクトップのみ）。監視スレッド(monitor)もこのプロセス内でしか
    // 動かないため、ここで弾けばアプリ本体・監視の両方が二重起動されなくなる。
    // 既に起動済みの場合は既存ウィンドウをフォアグラウンドに出して即終了する。
    #[cfg(not(target_os = "android"))]
    {
        if !platform::windows::ensure_single_instance() {
            return;
        }
    }

    // 起動時初期化: config.jsonからTHRESHOLD_SECSを読み込み
    let cfg = config::load_config_inner();
    if let Some(m) = cfg.idle_threshold_minutes {
        THRESHOLD_SECS.store(m as u64 * 60, Ordering::Relaxed);
    }

    let window = MainWindow::new().expect("ウィンドウの作成に失敗しました");
    window.set_app_version(format!("v{}", env!("CARGO_PKG_VERSION")).into());
    #[cfg(target_os = "android")]
    window.set_is_mobile(true);

    // 睡眠予測カードの初期入眠時刻 = 現在時刻（PredictionCard.tsx の currentHHMM 相当）。
    // 「今すぐ」ボタンを押したのと同じ状態なので、起動時からbedtime-sourceも"now"にして
    // ボタンの選択状態表示を実態と一致させる。
    {
        use chrono::Local;
        let now = Local::now();
        window.set_bed_hour(now.format("%H").to_string().parse().unwrap_or(22));
        window.set_bed_minute(now.format("%M").to_string().parse().unwrap_or(0));
        window.set_bedtime_source("now".into());
    }

    settings_ui::load_into_window(&window);

    let state = home::new_shared_state();
    home::refresh_all(&window, &state);

    // ── 睡眠予測カード ──
    {
        let weak = window.as_weak();
        window.on_bedtime_edited(move |_h, _m| {
            if let Some(w) = weak.upgrade() { home::recompute_prediction(&w); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_set_bedtime_now(move || {
            if let Some(w) = weak.upgrade() {
                use chrono::Local;
                let now = Local::now();
                w.set_bed_hour(now.format("%H").to_string().parse().unwrap_or(22));
                w.set_bed_minute(now.format("%M").to_string().parse().unwrap_or(0));
                home::recompute_prediction(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_set_bedtime_optimal(move || {
            if let Some(w) = weak.upgrade() {
                let sessions = events::get_sessions().unwrap_or_default();
                if sessions.is_empty() { return; }
                let cfg = config::load_config_inner();
                let target = cfg.target_wake_time;
                if let Some(opt) = prediction::find_optimal(&sessions, &home::now_iso(), target.as_deref()) {
                    let parts: Vec<&str> = opt.best_bed_time.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        w.set_bed_hour(parts[0].parse().unwrap_or(22));
                        w.set_bed_minute(parts[1].parse().unwrap_or(0));
                        home::recompute_prediction(&w);
                    }
                }
            }
        });
    }

    // ── 統計期間タブ ──
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_period_changed(move |key| {
            if let Some(w) = weak.upgrade() { home::set_period(&w, &s, &key); }
        });
    }

    // ── 週ナビゲーション ──
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_prev_week(move || {
            if let Some(w) = weak.upgrade() { home::adjust_week(&s, -7); home::update_chart(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_next_week(move || {
            if let Some(w) = weak.upgrade() { home::adjust_week(&s, 7); home::update_chart(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_this_week(move || {
            if let Some(w) = weak.upgrade() { home::reset_week_to_today(&s); home::update_chart(&w, &s); }
        });
    }

    // ── カレンダーピッカー ──
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_open_calendar(move || {
            if let Some(w) = weak.upgrade() { home::open_calendar(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_close_calendar(move || {
            if let Some(w) = weak.upgrade() { home::close_calendar(&w); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_cal_prev_month(move || {
            if let Some(w) = weak.upgrade() { home::cal_prev_month(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_cal_next_month(move || {
            if let Some(w) = weak.upgrade() { home::cal_next_month(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_cal_day_clicked(move |date| {
            if let Some(w) = weak.upgrade() { home::cal_select_day(&w, &s, &date); }
        });
    }

    // ── 日別詳細モーダル ──
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_day_clicked(move |date| {
            if let Some(w) = weak.upgrade() { home::open_day_detail(&w, &s, &date); }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_close_detail(move || {
            if let Some(w) = weak.upgrade() { home::close_day_detail(&w, &s); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_toggle_add_session(move || {
            if let Some(w) = weak.upgrade() {
                w.set_detail_add_open(!w.get_detail_add_open());
                w.set_detail_error("".into());
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_delete_session(move |start, end| {
            if let Some(w) = weak.upgrade() {
                match events::delete_session(start.to_string(), end.to_string()) {
                    Ok(()) => {
                        home::refresh_all(&w, &s);
                        if let Some(d) = home::selected_date(&s) { home::open_day_detail(&w, &s, &d); }
                    }
                    Err(e) => w.set_detail_error(format!("削除失敗: {}", e).into()),
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_add_session(move || {
            if let Some(w) = weak.upgrade() {
                let Some(date) = home::selected_date(&s) else { return };
                let start = format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:00",
                    w.get_detail_add_start_y(), w.get_detail_add_start_mo(), w.get_detail_add_start_d(),
                    w.get_detail_add_start_h(), w.get_detail_add_start_m(),
                );
                let end = format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:00",
                    w.get_detail_add_end_y(), w.get_detail_add_end_mo(), w.get_detail_add_end_d(),
                    w.get_detail_add_end_h(), w.get_detail_add_end_m(),
                );
                if start >= end {
                    w.set_detail_error("起床時刻は入眠時刻より後にしてください".into());
                    return;
                }
                match events::add_session(start, end) {
                    Ok(()) => { home::refresh_all(&w, &s); home::open_day_detail(&w, &s, &date); }
                    Err(e) => w.set_detail_error(format!("追加失敗: {}", e).into()),
                }
            }
        });
    }

    // ── 設定画面 ──
    {
        let weak = window.as_weak();
        window.on_toggle_startup(move |enable| {
            if let Some(w) = weak.upgrade() { settings_ui::toggle_startup(&w, enable); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_create_shortcut(move || {
            if let Some(w) = weak.upgrade() { settings_ui::create_shortcut(&w); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_save_settings(move || {
            if let Some(w) = weak.upgrade() { settings_ui::save(&w); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_test_connection(move || {
            if let Some(w) = weak.upgrade() {
                settings_ui::test_connection(w.as_weak(), w.get_mobile_url().to_string(), w.get_mobile_secret().to_string());
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_sync_now(move || {
            if let Some(w) = weak.upgrade() { settings_ui::sync_now(w.as_weak(), s.clone()); }
        });
    }
    {
        let weak = window.as_weak();
        window.on_export_csv(move || {
            settings_ui::export_csv(weak.clone());
        });
    }
    {
        let weak = window.as_weak();
        window.on_backup(move || {
            settings_ui::backup(weak.clone());
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_restore(move || {
            settings_ui::restore(weak.clone(), s.clone());
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_clear_all_data(move || {
            settings_ui::clear_all_data(weak.clone(), s.clone());
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_clear_all_data_and_cloud(move || {
            settings_ui::clear_all_data_and_cloud(weak.clone(), s.clone());
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_compact_data(move || {
            settings_ui::compact_data(weak.clone(), s.clone());
        });
    }

    // ── バックグラウンド処理 ──

    // 10秒ごとに現在時刻・起きてから経過時間を更新
    let timer = slint::Timer::default();
    {
        let weak = window.as_weak();
        let s = state.clone();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(10), move || {
            if let Some(w) = weak.upgrade() { home::apply_tick(&w, &s); }
        });
    }

    // Drive → ローカルへの起動時同期（別スレッド、完了後にUI再読み込み）
    {
        let weak = window.as_weak();
        let s = state.clone();
        std::thread::spawn(move || {
            cloud::ensure_events_from_drive();
            let _ = cloud::pull_mobile_events_inner();
            let weak = weak.clone();
            let s = s.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { home::refresh_all(&w, &s); }
            });
        });
    }

    // アイドル監視スレッド（Windowsデスクトップのみ）
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        let s = state.clone();
        platform::monitor::start(data_dir(), move || {
            let weak = weak.clone();
            let s = s.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() { home::refresh_all(&w, &s); }
            });
        });
    }

    // システムトレイ（Windowsデスクトップのみ）: 閉じるボタンでトレイに常駐
    #[cfg(windows)]
    platform::tray::setup(&window);

    // フォアグラウンド定期同期・起動時DEVICE_ON記録（Androidのみ）
    #[cfg(target_os = "android")]
    platform::android_bg::setup(&window, &state);

    // window.run() は「最後のウィンドウが隠れたら」イベントループごと終了してしまうため、
    // トレイに閉じるだけのWindowsデスクトップでは使えない
    // （閉じるボタン → HideWindow のつもりが、実質的にアプリごと終了してしまう）。
    // run_event_loop_until_quit() はトレイの「終了」メニューが呼ぶ quit_event_loop() まで
    // 生き続けるので、こちらを使う。
    window.show().expect("ウィンドウの表示に失敗しました");
    #[cfg(windows)]
    slint::run_event_loop_until_quit().expect("イベントループの実行に失敗しました");
    #[cfg(not(windows))]
    slint::run_event_loop().expect("イベントループの実行に失敗しました");
}
