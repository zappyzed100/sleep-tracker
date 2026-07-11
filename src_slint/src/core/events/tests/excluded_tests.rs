// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// excluded_tests.rs — 計測対象外の日（DAY_EXCLUDED/DAY_INCLUDED）のテスト
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : excluded_dates_from_content の判定ロジックと、その結果が
//        parse_sessions_from_str（Session.excluded）に正しく反映されるかを検証する。
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::core::events::Session;
use crate::core::events::excluded::excluded_dates_from_content;
use crate::core::events::parsing::parse_sessions_from_str;

const MIN: i64 = 60;

fn sessions(raw: &str, min_secs: i64) -> Vec<Session> {
    parse_sessions_from_str(raw, None, None, min_secs, MIN).0
}

fn sessions_with_manual(raw: &str, manual: &str, min_secs: i64) -> Vec<Session> {
    parse_sessions_from_str(raw, Some(manual), None, min_secs, MIN).0
}

#[test]
fn excluded_dates_from_content_finds_marked_date() {
    let raw = "2024-01-02 09:00:00,DAY_EXCLUDED:2024-01-01\n";
    let dates = excluded_dates_from_content(raw);
    assert!(dates.contains("2024-01-01"));
    assert_eq!(dates.len(), 1);
}

#[test]
fn excluded_dates_from_content_last_marker_wins_exclude_then_include() {
    let raw = "\
2024-01-02 09:00:00,DAY_EXCLUDED:2024-01-01
2024-01-03 09:00:00,DAY_INCLUDED:2024-01-01
";
    assert!(excluded_dates_from_content(raw).is_empty());
}

#[test]
fn excluded_dates_from_content_last_marker_wins_include_then_exclude() {
    let raw = "\
2024-01-02 09:00:00,DAY_INCLUDED:2024-01-01
2024-01-03 09:00:00,DAY_EXCLUDED:2024-01-01
";
    assert!(excluded_dates_from_content(raw).contains("2024-01-01"));
}

#[test]
fn session_on_excluded_day_is_still_returned_but_flagged() {
    // 開始時刻は睡眠日境界（午前4時）をまたがない09:00にする。00:00だと
    // sleep_day("2024-01-01 00:00:00")が前日(2023-12-31)になってしまい、
    // 「2024-01-01」を対象外にしても一致しない（is_excluded_at参照）。
    let raw = "\
2024-01-01 09:00:00,IDLE_START
2024-01-01 17:00:00,IDLE_RESUME
2024-01-02 00:00:00,DAY_EXCLUDED:2024-01-01
";
    let s = sessions(raw, MIN);
    // バーとしては表示し続けるため、除外してもセッション自体は消えない。
    assert_eq!(s.len(), 1);
    assert!(s[0].excluded);
}

#[test]
fn session_on_non_excluded_day_is_not_flagged() {
    let raw = "2024-01-01 09:00:00,IDLE_START\n2024-01-01 17:00:00,IDLE_RESUME\n";
    let s = sessions(raw, MIN);
    assert!(!s[0].excluded);
}

#[test]
fn excluding_one_day_does_not_affect_another_day_session() {
    let raw = "\
2024-01-01 09:00:00,IDLE_START
2024-01-01 17:00:00,IDLE_RESUME
2024-01-02 09:00:00,IDLE_START
2024-01-02 17:00:00,IDLE_RESUME
2024-01-03 00:00:00,DAY_EXCLUDED:2024-01-01
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 2);
    let day1 = s.iter().find(|x| x.start.starts_with("2024-01-01")).unwrap();
    let day2 = s.iter().find(|x| x.start.starts_with("2024-01-02")).unwrap();
    assert!(day1.excluded);
    assert!(!day2.excluded);
}

// ── 睡眠日境界をまたぐ除外判定（実データで見つかった回帰バグ） ───────────────────
// 深夜0〜4時開始のセッションは、暦日ではなく前日の睡眠日に属する
// （sleep_day境界は午前4時）。DAY_EXCLUDEDマーカーは常にUIが操作した睡眠日で
// 書き込まれるため、除外判定も睡眠日基準で一致させる必要がある
// （is_excluded_at参照。以前は暦日で比較しており、この境界をまたぐケースで
// 除外が効かないバグがあった）。
#[test]
fn session_starting_just_after_midnight_is_excluded_via_previous_sleep_day() {
    let raw = "\
2024-01-02 01:14:00,IDLE_START
2024-01-02 02:51:00,IDLE_RESUME
2024-01-03 00:00:00,DAY_EXCLUDED:2024-01-01
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert!(s[0].excluded);
}

#[test]
fn session_starting_just_after_midnight_is_not_excluded_by_its_calendar_date() {
    // 同じセッションを、暦日(2024-01-02)側で対象外にしても効かないことを確認する
    // （睡眠日ベースの判定が暦日と混同していない証拠）。
    let raw = "\
2024-01-02 01:14:00,IDLE_START
2024-01-02 02:51:00,IDLE_RESUME
2024-01-03 00:00:00,DAY_EXCLUDED:2024-01-02
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert!(!s[0].excluded);
}

#[test]
fn manual_session_on_excluded_day_is_flagged() {
    let raw = "2024-01-01 00:00:00,DAY_EXCLUDED:2024-01-01\n";
    let manual = "2024-01-01 13:00:00,2024-01-01 14:00:00\n";
    let s = sessions_with_manual(raw, manual, MIN);
    assert_eq!(s.len(), 1);
    assert!(s[0].excluded);
}
