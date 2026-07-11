//! config.rs — AppConfig の読み書きと Drive への設定同期
//!
//! 役割 : sleep-tracker の設定（睡眠判定閾値・モバイル URL・目標起床時刻など）を
//!        config.json ファイルへ保存・読み込みする。PC→Android 間の
//!        設定同期（push / fetch）も担当する。
//!        Tauri版 src-tauri/src/config.rs の移植（#[tauri::command] を除去しただけ）。
//!
//! 依存 : crate::data_dir, crate::config_path, crate::http_client,
//!        crate::THRESHOLD_SECS, super::events::SESSION_CACHE
//! 公開 : `AppConfig`, `get_config`, `save_config`, `fetch_settings_from_cloud`

use std::sync::atomic::Ordering;

const TAG: &str = "[config]";

// 「この時刻まで寝ていたら夜型」の基準時刻のデフォルト値（時、0-23）。
// utils::single_day_summaryの就寝/起床チャート用スケール変換に使う。
pub const NIGHT_TYPE_BOUNDARY_HOUR_DEFAULT: f64 = 14.0;

// SCREEN_ON区間のうち、これ未満は「一瞬触れただけ」として睡眠判定から無視する
// 最小時間のデフォルト値（分）。core::events::parsing::coalesce_and_filter_screen_on参照。
pub const MIN_SCREEN_ON_MINUTES_DEFAULT: u32 = 5;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct AppConfig {
    pub idle_threshold_minutes: Option<u32>,
    pub mobile_url: Option<String>,
    pub mobile_secret: Option<String>,
    pub target_wake_time: Option<String>,
    pub screen_on_enabled: Option<bool>,
    pub night_type_boundary_hour: Option<f64>,
    pub min_screen_on_minutes: Option<u32>,
}

// Subset of config synced between PC and Android via Drive.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SyncSettings {
    idle_threshold_minutes: Option<u32>,
    target_wake_time: Option<String>,
    night_type_boundary_hour: Option<f64>,
    min_screen_on_minutes: Option<u32>,
}

pub fn load_config_inner() -> AppConfig {
    let path = crate::config_path();
    if !path.exists() { return AppConfig::default(); }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn get_config() -> AppConfig {
    load_config_inner()
}

pub fn save_config(
    idle_threshold_minutes: u32,
    mobile_url: String,
    mobile_secret: String,
    target_wake_time: Option<String>,
    screen_on_enabled: Option<bool>,
    night_type_boundary_hour: Option<f64>,
    min_screen_on_minutes: u32,
) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let cfg = AppConfig {
        idle_threshold_minutes: Some(idle_threshold_minutes),
        mobile_url: if mobile_url.is_empty() { None } else { Some(mobile_url) },
        mobile_secret: if mobile_secret.is_empty() { None } else { Some(mobile_secret) },
        target_wake_time: target_wake_time.filter(|s| !s.is_empty()),
        screen_on_enabled,
        night_type_boundary_hour,
        min_screen_on_minutes: Some(min_screen_on_minutes),
    };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(crate::config_path(), json).map_err(|e| e.to_string())?;
    crate::THRESHOLD_SECS.store(idle_threshold_minutes as u64 * 60, Ordering::Relaxed);
    crate::MIN_SCREEN_ON_SECS.store(min_screen_on_minutes as u64 * 60, Ordering::Relaxed);
    *super::events::SESSION_CACHE.lock().unwrap() = None;

    let ms = t0.elapsed().as_millis();
    eprintln!("{} save_config: idle={}min  (+{}ms)", TAG, idle_threshold_minutes, ms);

    // Push shared settings to Drive in background so save_config returns immediately
    let cfg_clone = cfg.clone();
    std::thread::spawn(move || push_settings_to_drive_inner(&cfg_clone));
    Ok(())
}

// Push idle_threshold_minutes and target_wake_time to Drive (PC → Android).
// PC is always the source of truth; Android reads this on next fetch.
pub fn push_settings_to_drive_inner(cfg: &AppConfig) {
    let (base_url, secret) = match (&cfg.mobile_url, &cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u.clone(), s.clone()),
        _ => return,
    };
    let sync = SyncSettings {
        idle_threshold_minutes: cfg.idle_threshold_minutes,
        target_wake_time: cfg.target_wake_time.clone(),
        night_type_boundary_hour: cfg.night_type_boundary_hour,
        min_screen_on_minutes: cfg.min_screen_on_minutes,
    };
    let Ok(body) = serde_json::to_string(&sync) else { return };
    let url = format!("{}?secret={}&action=set_settings", base_url.trim_end_matches('/'), secret);
    let t0 = std::time::Instant::now();
    let result = crate::http_client().and_then(|c|
        c.post(&url).header("Content-Type", "application/json").body(body).send().map_err(|e| e.to_string())
    );
    let ms = t0.elapsed().as_millis();
    match result {
        Ok(_) => eprintln!("{} push_settings: sent  (+{}ms)", TAG, ms),
        Err(e) => eprintln!("{} ERROR push_settings: {}  (+{}ms)", TAG, e, ms),
    }
}

// Android: fetch shared settings from Drive and merge into local config.
// PC settings take priority — Android never pushes settings back.
pub fn fetch_settings_from_cloud() -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url.as_ref(), cfg.mobile_secret.as_ref()) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u.clone(), s.clone()),
        _ => return Ok(()),
    };
    let url = format!("{}?secret={}&action=get_settings", base_url.trim_end_matches('/'), secret);
    let resp = match crate::http_client().and_then(|c| c.get(&url).send().map_err(|e| e.to_string())) {
        Ok(r) if r.status().is_success() => r,
        _ => return Ok(()),
    };
    let text = resp.text().unwrap_or_default();
    if text.trim().is_empty() || text.trim() == "Unauthorized" || text.trim().starts_with("not found") {
        return Ok(());
    }
    let Ok(sync) = serde_json::from_str::<SyncSettings>(&text) else { return Ok(()) };
    let mut local = load_config_inner();
    if let Some(v) = sync.idle_threshold_minutes {
        local.idle_threshold_minutes = Some(v);
        crate::THRESHOLD_SECS.store(v as u64 * 60, Ordering::Relaxed);
        *super::events::SESSION_CACHE.lock().unwrap() = None;
    }
    if let Some(v) = sync.target_wake_time { local.target_wake_time = Some(v); }
    if let Some(v) = sync.night_type_boundary_hour { local.night_type_boundary_hour = Some(v); }
    if let Some(v) = sync.min_screen_on_minutes {
        local.min_screen_on_minutes = Some(v);
        crate::MIN_SCREEN_ON_SECS.store(v as u64 * 60, Ordering::Relaxed);
        *super::events::SESSION_CACHE.lock().unwrap() = None;
    }
    let json = serde_json::to_string_pretty(&local).map_err(|e| e.to_string())?;
    std::fs::write(crate::config_path(), json).map_err(|e| e.to_string())?;
    let ms = t0.elapsed().as_millis();
    let kb = text.len() as f64 / 1024.0;
    eprintln!("{} fetch_settings: {:.1}KB  (+{}ms)", TAG, kb, ms);
    Ok(())
}
