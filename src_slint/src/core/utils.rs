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

pub fn build_week(sessions: &[Session], week_base: NaiveDate) -> Vec<DaySummary> {
    let start = week_start(week_base);
    (0..7).map(|i| {
        let day = start + Duration::days(i);
        let next = day + Duration::days(1);
        let day_start = day.and_hms_opt(0, 0, 0).unwrap();
        let day_end = next.and_hms_opt(0, 0, 0).unwrap();

        let day_intervals: Vec<(NaiveDateTime, NaiveDateTime)> = sessions.iter()
            .filter_map(|s| {
                let st = parse_local(&s.start)?;
                let en = parse_local(&s.end)?;
                if st >= day_start && st < day_end { Some((st, en)) } else { None }
            })
            .collect();

        if day_intervals.is_empty() {
            return DaySummary { date: day, total_hours: 0.0, bedtime_h: None, waketime_h: None };
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
        }
    }).collect()
}
