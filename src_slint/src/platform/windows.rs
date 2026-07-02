//! platform.rs — Windows起動時自動起動・デスクトップショートカット
//!
//! 役割 : PC起動時のスタートアップ登録（レジストリRunキー）と
//!        デスクトップショートカット作成を担当する。
//!        Tauri版 lib.rs の該当部分の移植（#[tauri::command] を除去しただけ）。
//!
//! 公開 : `get_startup_enabled`, `set_startup`, `create_desktop_shortcut`

use std::path::PathBuf;

#[cfg(windows)]
const STARTUP_REG_VALUE: &str = "SleepTracker";

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

pub fn create_desktop_shortcut() -> Result<(), String> {
    #[cfg(windows)] {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let work_dir = exe.parent().unwrap_or(exe.as_path());
        let desktop = desktop_path()?;
        let lnk = desktop.join("睡眠トラッカー.lnk");
        let mut sl = mslnk::ShellLink::new(&exe).map_err(|e| e.to_string())?;
        sl.set_working_dir(Some(work_dir.to_string_lossy().into_owned()));
        sl.set_name(Some("睡眠トラッカー".into()));
        return sl.create_lnk(&lnk).map_err(|e| e.to_string());
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
