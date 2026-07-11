//! events.rs — sleep_events.txt のパース・セッション管理（エントリポイント）
//!
//! 役割 : Session/SessionCache の型定義と、実装を委譲する各サブモジュール
//!        （parsing/excluded/session_ops/backup/csv）の公開関数を
//!        束ねて再公開する。内部ファイルへの直接importは禁止、ここ経由でのみ使う。
//!
//! 依存 : crate::data_dir, crate::THRESHOLD_SECS, crate::core::cloud
//! 公開 : `Session`, `SessionCache`, `SESSION_CACHE`, `parse_sessions_rust`,
//!        `sort_events_file`, `sort_manual_file`, `get_sessions`, `add_session`, `delete_session`,
//!        `get_events_content`, `restore_events`, `clear_all_data`, `compact_data`,
//!        `current_sleep_start`, `maybe_auto_backup`, `clear_backups`, `list_backups`, `BackupEntry`,
//!        `export_csv`, `write_csv_file`, `import_csv`,
//!        `is_out_from_content`, `apply_mobile_event_line`,
//!        `excluded_dates_from_content`, `get_excluded_dates`, `set_day_excluded`

const TAG: &str = "[events]";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub start: String,
    pub end: String,
    #[serde(rename = "duration")]
    pub duration_hours: f64,
    #[serde(rename = "type")]
    pub session_type: String,
    // その日が計測対象外（平均睡眠・予測計算から除外）としてマークされているか。
    // 睡眠時間そのものは記録として残す（グラフのバーは表示する）が、
    // 統計計算だけから除外する。#[serde(default)]で既存データとの互換性を保つ。
    #[serde(default)]
    pub excluded: bool,
}

pub struct SessionCache {
    pub sessions: Vec<Session>,
    pub mtime: std::time::SystemTime,
}

pub static SESSION_CACHE: std::sync::Mutex<Option<SessionCache>> = std::sync::Mutex::new(None);

// sleep_events.txt への読み書きを保護するミューテックス。
// sort_events_file（全行読み込み→ソート→全行書き込み）と append_event（1行追加）の
// 競合によるデータ消失を防ぐ。
pub static EVENTS_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod parsing;
mod excluded;
mod session_ops;
mod backup;
mod csv;

#[cfg(test)]
mod tests {
    mod parsing_tests;
    mod excluded_tests;
    mod backup_tests;
}

pub use parsing::{
    is_out_from_content, apply_mobile_event_line, sort_events_file, sort_manual_file,
    parse_sessions_rust, get_sessions,
};
pub use excluded::{excluded_dates_from_content, get_excluded_dates, set_day_excluded};
pub use session_ops::{add_session, delete_session};
pub use backup::{
    get_events_content, restore_events, maybe_auto_backup, clear_backups, BackupEntry,
    list_backups, clear_all_data, current_sleep_start, compact_data,
};
pub use csv::{export_csv, write_csv_file, import_csv};
