// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// backup_tests.rs — detect_open_idle_and_out / extract_preserved_metadata_lines のテスト
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::core::events::backup::{detect_open_idle_and_out, extract_preserved_metadata_lines};

// ── detect_open_idle_and_out（暫定睡眠時間の判定に使う別関数）──────────────────

#[test]
fn detect_open_idle_and_out_reports_open_idle_start() {
    let raw = "2024-01-01 01:00:00,IDLE_START\n";
    let (idle, out) = detect_open_idle_and_out(raw);
    assert_eq!(idle.as_deref(), Some("2024-01-01 01:00:00"));
    assert_eq!(out, None);
}

#[test]
fn detect_open_idle_and_out_ignores_device_on_noise() {
    // これがまさに今回のバグの核心：DEVICE_ONが何度挟まっても、
    // detect_open_idle_and_out自体は元々DEVICE_ONを見ないため、
    // 「進行中」判定は最初から正しかった（バグはparse_sessions_from_str側だけにあった）。
    let raw = "\
2024-01-01 01:00:00,IDLE_START
2024-01-01 02:00:00,DEVICE_ON
2024-01-01 03:00:00,DEVICE_ON
";
    let (idle, _) = detect_open_idle_and_out(raw);
    assert_eq!(idle.as_deref(), Some("2024-01-01 01:00:00"));
}

#[test]
fn detect_open_idle_and_out_closed_session_reports_none() {
    let raw = "2024-01-01 01:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n";
    let (idle, _) = detect_open_idle_and_out(raw);
    assert_eq!(idle, None);
}

#[test]
fn detect_open_idle_and_out_reports_open_out() {
    let raw = "2024-01-01 01:00:00,OUT_START\n";
    let (_, out) = detect_open_idle_and_out(raw);
    assert_eq!(out.as_deref(), Some("2024-01-01 01:00:00"));
}

#[test]
fn detect_open_idle_and_out_in_house_closes_out() {
    let raw = "2024-01-01 01:00:00,OUT_START\n2024-01-01 02:00:00,IN_HOUSE\n";
    let (_, out) = detect_open_idle_and_out(raw);
    assert_eq!(out, None);
}

// 「データを圧縮」はUSAGE_APP_SEEN/ALLOWED/DENIEDと同様、DAY_EXCLUDED/DAY_INCLUDED
// （各日の計測対象外設定）もセッションでないため圧縮対象外とし、そのまま
// 引き継がなければならない（消すとユーザーが設定した計測対象外設定や
// アプリのON/OFF・検知履歴が圧縮のたびに失われてしまうため）。
#[test]
fn extract_preserved_metadata_lines_keeps_usage_and_day_excluded_and_ignores_sessions() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 08:00:00,IDLE_RESUME
2024-01-01 09:00:00,USAGE_APP_SEEN:com.android.chrome|Chrome
2024-01-01 09:00:01,USAGE_APP_DENIED:com.android.chrome
2024-01-01 09:00:02,USAGE_APP_ALLOWED:com.miui.home
2024-01-01 09:00:03,DAY_EXCLUDED:2024-01-01
2024-01-01 09:00:04,DAY_INCLUDED:2024-01-02
";
    let lines = extract_preserved_metadata_lines(raw);
    assert_eq!(lines.len(), 5);
    assert!(lines.iter().any(|l| l.contains("USAGE_APP_SEEN:com.android.chrome")));
    assert!(lines.iter().any(|l| l.contains("USAGE_APP_DENIED:com.android.chrome")));
    assert!(lines.iter().any(|l| l.contains("USAGE_APP_ALLOWED:com.miui.home")));
    assert!(lines.iter().any(|l| l.contains("DAY_EXCLUDED:2024-01-01")));
    assert!(lines.iter().any(|l| l.contains("DAY_INCLUDED:2024-01-02")));
    assert!(!lines.iter().any(|l| l.contains("IDLE_START")));
}
