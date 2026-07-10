//! excluded.rs — 計測対象外の日（DAY_EXCLUDED / DAY_INCLUDED）の管理
//!
//! 役割 : 「YYYY-MM-DD」を平均睡眠・予測計算から除外する設定の読み書き。
//!        マーカーは通常のイベント行と同じ書式でsleep_events.txtに追記され、
//!        同じ日付に対する最後のマーカーが有効になる。
//!
//! 依存 : super::{SESSION_CACHE}, super::parsing::sort_events_file, crate::core::cloud

use std::fs::OpenOptions;
use std::io::Write;

use super::SESSION_CACHE;
use super::parsing::sort_events_file;

// 「YYYY-MM-DD」を計測対象外（平均睡眠・予測計算から除外）としてマークされている
// 日付の集合として返す。マーカーは通常のイベント行と同じ書式
// "タイムスタンプ,DAY_EXCLUDED:YYYY-MM-DD" / "...,DAY_INCLUDED:YYYY-MM-DD" で
// ファイルに追記され、同じ日付に対する最後のマーカーが有効になる
// （タイムスタンプは実際に操作した時刻を使うため、ファイルソート後も
// 時系列どおりの「最後の操作が勝つ」が保たれる）。
pub fn excluded_dates_from_content(content: &str) -> std::collections::HashSet<String> {
    let mut state: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}');
        let Some(c) = line.find(',') else { continue };
        let rest = &line[c + 1..];
        if let Some(date) = rest.strip_prefix("DAY_EXCLUDED:") {
            state.insert(date.to_string(), true);
        } else if let Some(date) = rest.strip_prefix("DAY_INCLUDED:") {
            state.insert(date.to_string(), false);
        }
    }
    state.into_iter().filter(|(_, excluded)| *excluded).map(|(d, _)| d).collect()
}

// sleep_events.txtを直接読んで計測対象外の日付集合を返す。セッションが1件も
// 無い日（記録0h）でもチャート側で対象外表示ができるよう、Session一覧経由
// ではなくファイルから直接判定する（Session.excludedはセッションが存在する
// 日にしか付与されないため、0h の日はこちらでしか拾えない）。
pub fn get_excluded_dates() -> std::collections::HashSet<String> {
    let path = crate::data_dir().join("sleep_events.txt");
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    excluded_dates_from_content(&raw)
}

// 指定した日付(YYYY-MM-DD)を計測対象外/対象に切り替える。
pub fn set_day_excluded(date: &str, excluded: bool) -> Result<(), String> {
    let events_path = crate::data_dir().join("sleep_events.txt");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let marker = if excluded { "DAY_EXCLUDED" } else { "DAY_INCLUDED" };
    let line = format!("{},{}:{}\n", now, marker, date);
    let mut f = OpenOptions::new().create(true).append(true).open(&events_path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    drop(f);
    sort_events_file(&events_path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    let ep = events_path.clone();
    std::thread::spawn(move || { crate::core::cloud::auto_backup_after_event(&ep); });
    Ok(())
}
