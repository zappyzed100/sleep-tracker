//! utils.rs — 表示用フォーマット・週データ構築ユーティリティ
//!
//! 役割 : 時間・日付の表示用フォーマットと、週間チャート用の DaySummary 構築。
//!        Tauri版 core/utils.ts の移植。
//!
//! 公開 : `format_duration`, `DaySummary`, `week_start`, `build_week`

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};

use crate::Session;

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

pub struct DaySummary {
    pub date: NaiveDate,
    pub total_hours: f64,
    pub bedtime_h: Option<f64>,
    pub waketime_h: Option<f64>,
    pub excluded: bool,
}

fn parse_local(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()
}

// 月曜始まりの週の開始日を返す（chronoのweekday()はMon=0なのでそのまま使える）
pub fn week_start(ref_date: NaiveDate) -> NaiveDate {
    ref_date - Duration::days(ref_date.weekday().num_days_from_monday() as i64)
}

// 0時〜11:59は「深夜」として+24hしたスケールに変換する（就寝/起床チャート用）
fn to_night_hour(dt: NaiveDateTime) -> f64 {
    let h = dt.hour() as f64 + dt.minute() as f64 / 60.0;
    if h < 12.0 { h + 24.0 } else { h }
}

// PC版・Android版それぞれが記録した睡眠時間が重なっている場合、単純に合計すると
// 二重計上になる（例: 0:00-8:00 と 4:00-10:00 は合計14hではなく、繋げて10h）。
// 重複・隣接する区間を1本にマージしてから合計・就寝起床時刻を算出する。
pub fn merge_intervals(mut intervals: Vec<(NaiveDateTime, NaiveDateTime)>) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    intervals.sort_by_key(|(s, _)| *s);
    let mut merged: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::new();
    for (s, e) in intervals {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                if e > last.1 { last.1 = e; }
                continue;
            }
        }
        merged.push((s, e));
    }
    merged
}

pub fn build_week(
    sessions: &[Session],
    week_base: NaiveDate,
    excluded_dates: &std::collections::HashSet<String>,
) -> Vec<DaySummary> {
    let start = week_start(week_base);
    (0..7).map(|i| {
        let day = start + Duration::days(i);
        let next = day + Duration::days(1);
        let day_start = day.and_hms_opt(0, 0, 0).unwrap();
        let day_end = next.and_hms_opt(0, 0, 0).unwrap();

        let day_sessions: Vec<&Session> = sessions.iter()
            .filter(|s| parse_local(&s.start).is_some_and(|st| st >= day_start && st < day_end))
            .collect();
        // セッションが1件も無い日（記録0h）でも計測対象外表示ができるよう、
        // セッション側のexcludedフラグだけでなく、ファイルの除外マーカーも直接見る。
        let excluded = day_sessions.iter().any(|s| s.excluded)
            || excluded_dates.contains(&day.format("%Y-%m-%d").to_string());

        let day_intervals: Vec<(NaiveDateTime, NaiveDateTime)> = day_sessions.iter()
            .filter_map(|s| Some((parse_local(&s.start)?, parse_local(&s.end)?)))
            .collect();

        if day_intervals.is_empty() {
            return DaySummary { date: day, total_hours: 0.0, bedtime_h: None, waketime_h: None, excluded };
        }

        let merged = merge_intervals(day_intervals);
        let total: f64 = merged.iter().map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0).sum();
        let (bedtime, waketime) = *merged.iter()
            .max_by(|a, b| (a.1 - a.0).cmp(&(b.1 - b.0)))
            .unwrap();

        DaySummary {
            date: day,
            total_hours: total,
            bedtime_h: Some(to_night_hour(bedtime)),
            waketime_h: Some(to_night_hour(waketime)),
            excluded,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(start: &str, end: &str, excluded: bool) -> Session {
        Session {
            start: start.to_string(),
            end: end.to_string(),
            duration_hours: 0.0,
            session_type: "IDLE".to_string(),
            excluded,
        }
    }

    #[test]
    fn day_with_session_flagged_excluded_is_marked_excluded_in_summary() {
        let sessions = vec![session("2024-01-01 00:00:00", "2024-01-01 08:00:00", true)];
        let empty = std::collections::HashSet::new();
        let days = build_week(&sessions, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), &empty);
        let day1 = days.iter().find(|d| d.date == NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()).unwrap();
        assert!(day1.excluded);
    }

    #[test]
    fn day_with_no_sessions_but_excluded_marker_is_still_marked_excluded() {
        // 回帰テスト：セッションが1件も無い日（記録0h）でも、除外マーカーが
        // あればグラフ側で対象外だと分かるようにする（以前はセッション経由でしか
        // 判定していなかったため、0hの日は対象外にしても何も表示されなかった）。
        let sessions: Vec<Session> = vec![];
        let mut excluded_dates = std::collections::HashSet::new();
        excluded_dates.insert("2024-01-03".to_string());
        let days = build_week(&sessions, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), &excluded_dates);
        let day3 = days.iter().find(|d| d.date == NaiveDate::from_ymd_opt(2024, 1, 3).unwrap()).unwrap();
        assert!(day3.excluded);
        assert_eq!(day3.total_hours, 0.0);
    }

    #[test]
    fn day_without_marker_or_excluded_session_is_not_excluded() {
        let sessions = vec![session("2024-01-01 00:00:00", "2024-01-01 08:00:00", false)];
        let empty = std::collections::HashSet::new();
        let days = build_week(&sessions, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), &empty);
        let day1 = days.iter().find(|d| d.date == NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()).unwrap();
        assert!(!day1.excluded);
    }
}
