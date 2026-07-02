//! utils.rs — 表示用フォーマットユーティリティ
//!
//! 役割 : 時間・日付の表示用フォーマット。Tauri版 core/utils.ts の formatDuration 相当。
//!
//! 公開 : `format_duration`

// 例: 7.5 → "7h30m"、7.0 → "7h"
pub fn format_duration(hours: f64) -> String {
    let total_min = (hours * 60.0).round() as i64;
    let h = total_min / 60;
    let m = total_min % 60;
    if m == 0 {
        format!("{}h", h)
    } else {
        format!("{}h{}m", h, m)
    }
}
