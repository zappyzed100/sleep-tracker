//! platform/android/mod.rs — Android版プラットフォーム機能のエントリポイント
//!
//! 役割 : Android専用の3モジュール（エントリポイント・フォアグラウンド同期・
//!        バックアップ復元）を束ね、外部（lib.rs・ui/）から使う関数だけを再公開する。
//!
//! 依存 : crate::{core, ui}
//! 公開 : `setup`, `activity`, `launch_picker`

mod entry;
mod bg;
mod restore;

pub use bg::{activity, setup};
pub use restore::launch_picker;
