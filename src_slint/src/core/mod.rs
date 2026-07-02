//! core/mod.rs — ビジネスロジック層のエントリポイント
//!
//! 役割 : config/events/cloud/prediction/utils を束ね、`Session` をcrate直下へ
//!        再公開する（PC/Android両方のUI層から `crate::Session` で参照できるように）。
//!
//! 公開 : `config`, `events`, `cloud`, `prediction`, `utils`, `Session`

pub mod config;
pub mod events;
pub mod cloud;
pub mod prediction;
pub mod utils;

pub use events::Session;
