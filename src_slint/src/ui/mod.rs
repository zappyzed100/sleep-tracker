//! ui/mod.rs — 画面ロジック層のエントリポイント
//!
//! 役割 : ホーム画面（統計・予測・チャート・日別詳細）と設定画面の
//!        Slint連携ロジックを束ねる。
//!
//! 公開 : `home`, `settings_ui`, `sync_status`

pub mod home;
pub mod settings_ui;
pub mod sync_status;
