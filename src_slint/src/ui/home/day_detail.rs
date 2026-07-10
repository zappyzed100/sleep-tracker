//! day_detail.rs — 日別詳細モーダル（DayDetail.tsx 相当）
//!
//! 役割 : 特定の日（睡眠日基準）のセッション一覧表示、計測対象外への切り替えを担当する。
//!
//! 依存 : super::{state::SharedState, chart::update_chart, stats::compute_stats},
//!        crate::core::{events, utils, Session}, crate::{MainWindow, SessionVM}

use super::chart::update_chart;
use super::stats::compute_stats;
use super::state::SharedState;
use crate::core::{events, utils, Session};
use crate::{MainWindow, SessionVM};
use chrono::{Datelike, NaiveDate};

fn date_label_ja(d: NaiveDate) -> String {
    const DOW_SUN_FIRST: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
    format!("{}年{}月{}日（{}）", d.year(), d.month(), d.day(), DOW_SUN_FIRST[d.weekday().num_days_from_sunday() as usize])
}

fn fmt_ts_short(ts: &str) -> String {
    // "2026-07-01 23:00:00" → "7/1 23:00"
    let (date, time) = ts.split_once(' ').unwrap_or((ts, ""));
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    if parts.len() == 3 {
        format!("{}/{} {}", parts[1].trim_start_matches('0'), parts[2].trim_start_matches('0'), time.get(..5).unwrap_or(time))
    } else {
        ts.to_string()
    }
}

pub fn open_day_detail(window: &MainWindow, state: &SharedState, date: &str) {
    state.lock().unwrap().selected_date = Some(date.to_string());
    window.set_detail_excluded_message("".into());

    let target_date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
    let sessions = events::get_sessions().unwrap_or_default();
    // チャート側(build_week)と同じ「睡眠日」基準で絞り込む。暦日の前方一致だと、
    // 深夜1:33開始の睡眠が前日バーに計上されているのにクリックしても出てこない、
    // というズレが起きるため。
    let day_sessions: Vec<&Session> = sessions.iter().filter(|s| {
        chrono::NaiveDateTime::parse_from_str(s.start.trim(), "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(utils::sleep_day) == target_date
    }).collect();
    // セッションが1件も無い日（記録0h）でもボタンの状態が正しく反映されるよう、
    // セッション側のフラグだけでなくファイルの除外マーカーも直接見る
    // （そうしないと0hの日では「対象外にする」を押しても何も反応しないように見えるバグになる）。
    window.set_detail_excluded(
        day_sessions.iter().any(|s| s.excluded) || events::get_excluded_dates().contains(date)
    );

    // PC/Android両方から記録された重複区間は合計計算では1本にまとめて二重計上を
    // 防ぐ（一覧表示では元のセッションをそのまま出し、重複しているものには
    // ⚠マークを付ける）。
    let parse = |s: &str| chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok();
    let day_intervals: Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)> = day_sessions.iter()
        .filter_map(|s| Some((parse(&s.start)?, parse(&s.end)?)))
        .collect();
    let merged = utils::merge_intervals(day_intervals.clone());
    let total: f64 = merged.iter().map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0).sum();

    let vm: Vec<SessionVM> = day_sessions.iter().enumerate().map(|(i, s)| {
        let overlaps = match (parse(&s.start), parse(&s.end)) {
            (Some(st), Some(en)) => day_intervals.iter().enumerate()
                .any(|(j, (os, oe))| j != i && st < *oe && en > *os),
            _ => false,
        };
        SessionVM {
            start: s.start.clone().into(),
            end: s.end.clone().into(),
            time_range: format!("{} → {}", fmt_ts_short(&s.start), fmt_ts_short(&s.end)).into(),
            duration_label: utils::format_duration(s.duration_hours).into(),
            deleting: false,
            overlaps,
        }
    }).collect();

    let d = target_date.unwrap_or_else(|| chrono::Local::now().date_naive());
    let next = d + chrono::Duration::days(1);
    window.set_detail_date_label(date_label_ja(d).into());
    window.set_detail_total_label(utils::format_duration(total).into());
    window.set_detail_sessions(slint::ModelRc::new(slint::VecModel::from(vm)));
    window.set_detail_add_open(false);
    window.set_detail_error("".into());
    window.set_detail_add_start_y(d.year());
    window.set_detail_add_start_mo(d.month() as i32);
    window.set_detail_add_start_d(d.day() as i32);
    window.set_detail_add_start_h(23);
    window.set_detail_add_start_m(0);
    window.set_detail_add_end_y(next.year());
    window.set_detail_add_end_mo(next.month() as i32);
    window.set_detail_add_end_d(next.day() as i32);
    window.set_detail_add_end_h(7);
    window.set_detail_add_end_m(0);
    window.set_show_detail(true);

    update_chart(window, state);
}

pub fn close_day_detail(window: &MainWindow, state: &SharedState) {
    state.lock().unwrap().selected_date = None;
    window.set_show_detail(false);
    update_chart(window, state);
}

// 日別詳細モーダルで開いている日を計測対象外/対象に切り替える。
pub fn toggle_day_excluded(window: &MainWindow, state: &SharedState) {
    let date = state.lock().unwrap().selected_date.clone();
    let Some(date) = date else { return };
    let now_excluded = window.get_detail_excluded();
    let new_excluded = !now_excluded;
    if let Err(e) = events::set_day_excluded(&date, new_excluded) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        window.set_detail_excluded_message(format!("失敗: {} ({})", e, now).into());
        eprintln!("[app] toggle_day_excluded: ERROR {}", e);
        return;
    }
    // open_day_detail は呼び出し直後に detail-excluded-message をクリアするため、
    // 確認メッセージは必ずopen_day_detail呼び出しの後で設定すること。
    open_day_detail(window, state, &date);
    compute_stats(window, state);
    let now = chrono::Local::now().format("%H:%M:%S").to_string();
    let msg = if new_excluded { "✓ 計測対象外にしました" } else { "✓ 計測対象に戻しました" };
    window.set_detail_excluded_message(format!("{} ({})", msg, now).into());
}
