//! platform/android/mod.rs — Android版プラットフォーム機能のエントリポイント
//!
//! 役割 : Android専用の4モジュール（エントリポイント・フォアグラウンド同期・
//!        バックアップ復元・アプリ使用状況ブリッジ）を束ね、外部（lib.rs・ui/）
//!        から使う関数だけを再公開する。
//!
//! 依存 : crate::{core, ui}
//! 公開 : `setup`, `activity`, `refresh_ui`, `launch_picker`

mod entry;
mod bg;
mod restore;
mod usage;

pub use bg::{activity, refresh_ui, setup};
pub use restore::launch_picker;
