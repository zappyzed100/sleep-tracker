//! tray.rs — Windowsシステムトレイ（最小化してトレイに常駐）
//!
//! 役割 : トレイアイコン＋メニュー（開く/終了）を作成し、ウィンドウを閉じた際に
//!        プロセスを終了せずトレイに隠す。tray-iconクレートはwinitイベントループに
//!        依存しないポーリング用チャンネル（TrayIconEvent::receiver / MenuEvent::receiver）
//!        を提供しているため、SlintのTimerで定期的にポーリングして処理する。
//!        同じタイマーで、二重起動時に他プロセスから送られる「表示して」通知
//!        （platform::windows::ensure_single_instance）も監視する。
//!        Tauri版のトレイ機能（開く/終了メニュー、閉じるボタンで最小化）の移植。
//!
//! 公開 : `setup(window: &MainWindow)`

use crate::platform::windows as win;
use crate::MainWindow;
use slint::ComponentHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

fn load_icon() -> tray_icon::Icon {
    let bytes = include_bytes!("../../assets/tray-icon.png");
    let img = image::load_from_memory(bytes).expect("tray-icon.png のデコードに失敗しました").into_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).expect("トレイアイコンの作成に失敗しました")
}

pub fn setup(window: &MainWindow) {
    let open_item = MenuItem::with_id("open", "開く", true, None);
    let quit_item = MenuItem::with_id("quit", "終了", true, None);
    let menu = Menu::new();
    let _ = menu.append(&open_item);
    let _ = menu.append(&quit_item);

    // TrayIcon はドロップされるとアイコンが消えるため、Box::leak でプロセス終了まで保持する。
    let tray: &'static TrayIcon = Box::leak(Box::new(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(load_icon())
            .with_tooltip("睡眠トラッカー")
            .build()
            .expect("トレイアイコンの作成に失敗しました"),
    ));
    let _ = tray;

    // 閉じるボタン → 終了せずウィンドウを隠す
    window.window().on_close_requested(|| slint::CloseRequestResponse::HideWindow);

    // トレイアイコン/メニューのイベントはポーリング専用チャンネルで受け取る
    // （winitイベントループに直接フックしない、Slintの標準runループと共存させるため）。
    let tray_rx = TrayIconEvent::receiver();
    let menu_rx = MenuEvent::receiver();
    let wake_event = win::create_wake_event();
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(150), move || {
        let Some(w) = weak.upgrade() else { return };

        while let Ok(event) = tray_rx.try_recv() {
            if matches!(event, TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. }) {
                let _ = w.window().show();
            }
        }
        while let Ok(event) = menu_rx.try_recv() {
            match event.id.0.as_str() {
                "open" => { let _ = w.window().show(); }
                "quit" => { let _ = slint::quit_event_loop(); }
                _ => {}
            }
        }
        // 二重起動されたときに他プロセスから送られる「表示して」通知。
        // トレイに隠れている状態でOSのShowWindowを直接呼ぶとSlint内部の表示状態と
        // 食い違い真っ白な画面になっていたため、必ず自分自身のwindow().show()経由で表示する。
        if win::wake_event_signaled(wake_event) {
            let _ = w.window().show();
        }
    });
    // timer をリークして保持する（main() のスコープを抜けても動き続けるように）。
    std::mem::forget(timer);
}
