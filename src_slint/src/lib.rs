//! lib.rs — sleep_tracker (Rust + Slint) 共有ロジック・エントリポイント
//!
//! 役割 : デスクトップ（src/main.rs）・Android（src/platform/android/entry.rs）の
//!        両方から呼ばれる共通ロジック。core/（ビジネスロジック）・platform/（OS固有
//!        機能）・ui/（画面ロジック）・paths/（データ/設定パス解決）を宣言し、
//!        Slintウィンドウの起動と全コールバックの配線(`run`)を行う。
//!
//! 公開 : `run`, `THRESHOLD_SECS`, `data_dir`, `config_path`, `http_client`,
//!        `Session`, `init_android_app_dir`（Android専用）

mod core;
mod platform;
mod ui;
mod paths;

use core::{cloud, config, events, prediction};
use ui::{home, settings_ui};

pub use core::Session;
pub use paths::*;

use std::sync::atomic::Ordering;

slint::include_modules!();

// Drive → ローカルへの同期を1回実行する（別スレッド、完了後にUI再読み込み）。
// 起動時と、PCがフォアグラウンドに戻った時の両方から呼ばれる共通処理。
// 「同期を停止」中は自動同期のこの経路をスキップする（手動の「今すぐ同期」
// ボタンはこのフラグを見ないので、停止中でも明示的に押せば同期できる）。
fn drive_sync(weak: slint::Weak<MainWindow>, state: home::SharedState) {
    if cloud::is_sync_paused() { return; }
    ui::sync_status::begin(&weak);
    std::thread::spawn(move || {
        cloud::ensure_events_from_drive();
        let _ = cloud::pull_mobile_events_inner();
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        let weak2 = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak2.upgrade() { home::refresh_all(&w, &state); }
        });
        ui::sync_status::end(&weak, Some((format!("✓ 同期完了 ({})", now), "success")));
    });
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
                let sessions: Vec<Session> = events::get_sessions().unwrap_or_default()
                    .into_iter().filter(|s| !s.excluded).collect();
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
        let s = state.clone();
        window.on_toggle_day_excluded(move || {
            if let Some(w) = weak.upgrade() { home::toggle_day_excluded(&w, &s); }
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
        window.on_close_backup_list(move || {
            if let Some(w) = weak.upgrade() {
                settings_ui::close_backup_list(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_restore_from_backup(move |path| {
            settings_ui::restore_from_backup(weak.clone(), s.clone(), path.to_string());
        });
    }
    {
        let weak = window.as_weak();
        let s = state.clone();
        window.on_restore_via_external_picker(move || {
            settings_ui::restore_via_external_picker(weak.clone(), s.clone());
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
    {
        let weak = window.as_weak();
        window.on_clear_backups(move || {
            settings_ui::clear_backups(weak.clone());
        });
    }
    {
        let weak = window.as_weak();
        window.on_toggle_sync_paused(move || {
            if let Some(w) = weak.upgrade() {
                settings_ui::clear_stale_confirmations(&w, "");
                let new_state = !cloud::is_sync_paused();
                if let Err(e) = cloud::set_sync_paused(new_state) {
                    eprintln!("[app] toggle_sync_paused: ERROR {}", e);
                    return;
                }
                w.set_sync_paused(new_state);
            }
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

    // Drive → ローカルへの起動時同期（別スレッド、完了後にUI再読み込み）。
    // 同期アイコン(sync-in-progress)も手動同期ボタンと同じ見た目で回転させる
    // （起動時に実際に同期しているのにアイコンが動かず不安、という指摘への対応）。
    drive_sync(window.as_weak(), state.clone());

    // ウィンドウがフォアグラウンドに戻った（=人間が操作を再開した）瞬間にも同期する
    // （トレイからの復帰・タスクバーからの復帰・他ウィンドウからのAlt+Tab切り替え、
    // いずれもここで検知できる）。既存の10秒tickに相乗りし、前回tick時との
    // フォアグラウンド状態の変化だけを見る。起動直後は上のdrive_syncと二重に
    // 走らないよう、初期値をtrue（=起動時点でフォアグラウンド）にしておく。
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        let s = state.clone();
        let mut was_foreground = true;
        let fg_timer = slint::Timer::default();
        fg_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(10), move || {
            let Some(w) = weak.upgrade() else { return };
            let is_fg = platform::windows::is_foreground(w.window());
            if is_fg && !was_foreground {
                drive_sync(weak.clone(), s.clone());
            }
            was_foreground = is_fg;
        });
        std::mem::forget(fg_timer);
    }

    // アイドル監視スレッド（Windowsデスクトップのみ）
    #[cfg(windows)]
    {
        let weak = window.as_weak();
        let s = state.clone();
        platform::monitor::start(data_dir(), window.as_weak(), move || {
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

    // フォアグラウンド定期同期・onResume()経由の同期キック（Androidのみ）
    #[cfg(target_os = "android")]
    platform::android::setup(&window, &state);

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
