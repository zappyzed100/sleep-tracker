//! platform/mod.rs — OS固有機能のエントリポイント
//!
//! 役割 : Windows（起動設定・アイドル監視・システムトレイ）とAndroid
//!        （エントリポイント・フォアグラウンド同期）のプラットフォーム固有実装を束ねる。
//!
//! 公開 : `windows`, `monitor`（Windowsのみ）, `tray`（Windowsのみ）,
//!        `android`, `android_bg`, `android_restore`, `android_usage`（Androidのみ）

pub mod windows;
#[cfg(windows)]
pub mod monitor;
#[cfg(windows)]
pub mod tray;
#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "android")]
pub mod android_bg;
#[cfg(target_os = "android")]
pub mod android_restore;
#[cfg(target_os = "android")]
pub mod android_usage;
