//! platform.rs — Windows起動時自動起動・デスクトップショートカット・多重起動防止
//!
//! 役割 : PC起動時のスタートアップ登録（レジストリRunキー）と
//!        デスクトップショートカット作成を担当する。
//!        Tauri版 lib.rs の該当部分の移植（#[tauri::command] を除去しただけ）。
//!        多重起動防止は名前付きMutexで実現し、監視スレッド(monitor)もこの
//!        プロセス内でしか動かないため、これだけでアプリ本体・監視の両方が
//!        二重起動されないようになる。
//!
//! 公開 : `get_startup_enabled`, `set_startup`, `create_desktop_shortcut`,
//!        `ensure_single_instance`, `bring_to_foreground`, `is_foreground`

use std::path::PathBuf;

#[cfg(windows)]
const STARTUP_REG_VALUE: &str = "SleepTracker";

#[cfg(windows)]
const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\SleepTrackerSingleInstanceMutex";

#[cfg(windows)]
const WAKE_EVENT_NAME: &str = "Local\\SleepTrackerWakeEvent";

// アプリの多重起動を防ぐ。名前付きMutexが既に存在する（＝他のインスタンスが
// 起動済み）場合はfalseを返す。呼び出し側はfalseが返ったら即座にプロセスを
// 終了すること。
//
// 既存インスタンスの呼び出しには、以前はFindWindowW+ShowWindow+SetForegroundWindow
// を直接使っていたが、このアプリはトレイに閉じるとSlintの`HideWindow`でウィンドウを
// 隠すため、Win32のShowWindowだけで無理やり表示するとSlint内部の表示状態と
// 食い違い、真っ白なウィンドウになる不具合があった。名前付きイベントで既存プロセス
// 自身に「表示して」と伝え、既存プロセスが自分の`window().show()`を呼ぶ方式にする
// （tray.rsのポーリングタイマーで受信）。
//
// Mutexハンドルはプロセス終了までホールドし続ける必要があるため、
// 意図的にリーク（forget）する — 通常のOSはプロセス終了時に自動で解放する。
pub fn ensure_single_instance() -> bool {
    #[cfg(windows)] {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent};

        let name: Vec<u16> = SINGLE_INSTANCE_MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        if already_running {
            if handle != 0 {
                unsafe { CloseHandle(handle); }
            }
            let ev_name: Vec<u16> = WAKE_EVENT_NAME.encode_utf16().chain(std::iter::once(0)).collect();
            let ev = unsafe { CreateEventW(std::ptr::null(), 0, 0, ev_name.as_ptr()) };
            if ev != 0 {
                unsafe {
                    SetEvent(ev);
                    CloseHandle(ev);
                }
            }
            return false;
        }

        // HANDLE(isize)はCloseHandleを呼ばない限りOS側で保持され続けるため、
        // ここで意図的にCloseHandleを呼ばず、プロセス終了までMutexを保持させる。
        return true;
    }
    #[allow(unreachable_code)]
    true
}

// 「表示して」通知を受け取るためのイベントハンドルを作る（プライマリインスタンス側
// で起動時に1回呼ぶ）。CreateEventWは名前が既存でも成功して同じオブジェクトの
// ハンドルを返すため、作成順は問わない。戻り値はtray.rsのポーリングタイマーに
// 渡してwake_event_signaledで監視する。
pub fn create_wake_event() -> isize {
    #[cfg(windows)] {
        use windows_sys::Win32::System::Threading::CreateEventW;
        let ev_name: Vec<u16> = WAKE_EVENT_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        return unsafe { CreateEventW(std::ptr::null(), 0, 0, ev_name.as_ptr()) };
    }
    #[allow(unreachable_code)]
    {
        0
    }
}

// 他プロセスがensure_single_instance()経由でイベントをSetした直後かどうかを
// 非ブロッキングで確認する（自動リセットイベントなので成功したら次はfalseに戻る）。
pub fn wake_event_signaled(handle: isize) -> bool {
    #[cfg(windows)] {
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        const WAIT_OBJECT_0: u32 = 0;
        return handle != 0 && unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
    }
    #[allow(unreachable_code)]
    {
        let _ = handle;
        false
    }
}

// slintの`Window::show()`は非表示から可視化するだけで、最前面に持ってきたり
// フォーカスを与えたりはしない。そのためトレイの「開く」や二重起動の
// 「表示して」通知でshow()だけ呼んでも、他のウィンドウの裏に隠れたままで
// 「反応していないように見える」ことがあった。raw-window-handle経由で実際の
// HWNDを取得し、SetForegroundWindowで確実に前面へ出す。
// show()を先に呼んでSlint側の表示状態を正しくした後にこれを呼ぶこと。
pub fn bring_to_foreground(window: &slint::Window) {
    #[cfg(windows)] {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};

        let slint_handle = window.window_handle();
        let Ok(handle) = slint_handle.window_handle() else { return };
        if let RawWindowHandle::Win32(h) = handle.as_raw() {
            let hwnd: isize = h.hwnd.into();
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

// ウィンドウが現在フォアグラウンド（アクティブ）かどうかを判定する。
// トレイ復帰・タスクバー復帰・Alt+Tab切り替えなど、経路を問わず「今アクティブか」
// だけを見たいので、個々の復帰経路をフックするのではなく毎tickポーリングする方式にした。
pub fn is_foreground(window: &slint::Window) -> bool {
    #[cfg(windows)] {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

        let slint_handle = window.window_handle();
        let Ok(handle) = slint_handle.window_handle() else { return false };
        if let RawWindowHandle::Win32(h) = handle.as_raw() {
            let hwnd: isize = h.hwnd.into();
            return unsafe { GetForegroundWindow() } == hwnd;
        }
    }
    #[allow(unreachable_code)]
    {
        let _ = window;
        false
    }
}

pub fn get_startup_enabled() -> bool {
    #[cfg(windows)] {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        return hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .and_then(|key| key.get_value::<String, _>(STARTUP_REG_VALUE))
            .is_ok();
    }
    #[allow(unreachable_code)]
    false
}

pub fn set_startup(enable: bool) -> Result<(), String> {
    let _ = enable;
    #[cfg(windows)] {
        use winreg::{RegKey, enums::{HKEY_CURRENT_USER, KEY_WRITE}};
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run = hkcu
            .open_subkey_with_flags("Software\\Microsoft\\Windows\\CurrentVersion\\Run", KEY_WRITE)
            .map_err(|e| e.to_string())?;
        return if enable {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            run.set_value(STARTUP_REG_VALUE, &exe.to_string_lossy().as_ref())
                .map_err(|e| e.to_string())
        } else {
            run.delete_value(STARTUP_REG_VALUE).or(Ok(()))
        };
    }
    #[allow(unreachable_code)]
    Ok(())
}

// 戻り値: Ok(true) = 新規作成、Ok(false) = 既存のショートカットを上書き。
// 呼び出し側がどちらの場合も必ずメッセージを出せるよう、既に存在していたかどうかを
// 呼び出し前に判定して返す（mslnkのcreate_lnkはOSのCreateFile相当で常に上書き
// 成功してしまい、既存だったかどうかがOk/Errだけでは分からないため）。
pub fn create_desktop_shortcut() -> Result<bool, String> {
    #[cfg(windows)] {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let work_dir = exe.parent().unwrap_or(exe.as_path());
        let desktop = desktop_path()?;
        let lnk = desktop.join("睡眠トラッカー.lnk");
        let already_existed = lnk.exists();
        let mut sl = mslnk::ShellLink::new(&exe).map_err(|e| e.to_string())?;
        sl.set_working_dir(Some(work_dir.to_string_lossy().into_owned()));
        sl.set_name(Some("睡眠トラッカー".into()));
        sl.create_lnk(&lnk).map_err(|e| e.to_string())?;
        return Ok(!already_existed);
    }
    #[allow(unreachable_code)]
    Err("デスクトップショートカットは Windows のみサポートしています".into())
}

#[cfg(windows)]
fn desktop_path() -> Result<PathBuf, String> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders"
    ) {
        if let Ok(path) = key.get_value::<String, _>("Desktop") {
            let p = PathBuf::from(&path);
            if p.exists() { return Ok(p); }
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        for name in &["Desktop", "デスクトップ"] {
            let p = PathBuf::from(&profile).join(name);
            if p.exists() { return Ok(p); }
        }
    }
    Err("デスクトップフォルダが見つかりません".into())
}
