// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// events_tests.rs — 睡眠判定（parse_sessions_from_str）のテスト
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : sleep_events.txt のパース・セッション再構築ロジックを、
//        実ファイル・実時刻に依存しない純粋関数(parse_sessions_from_str等)
//        に対して直接検証する。日付はすべて架空の値（2024-01-01〜）を使い、
//        実データの日付は含めない。
//
// 依存 : super::*（events.rs 内の非公開関数を含む）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use super::*;

const MIN: i64 = 60; // テスト用のゆるいしきい値（1分）。特にことわりが無い限りこれを使う。

fn sessions(raw: &str, min_secs: i64) -> Vec<Session> {
    parse_sessions_from_str(raw, None, None, min_secs).0
}

fn sessions_with_manual(raw: &str, manual: &str, min_secs: i64) -> Vec<Session> {
    parse_sessions_from_str(raw, Some(manual), None, min_secs).0
}

fn hours(s: &Session) -> f64 { (s.duration_hours * 100.0).round() / 100.0 }

// ── 基本のIDLE_START/IDLE_RESUMEペア ──────────────────────────────────────────

#[test]
fn empty_file_returns_no_sessions() {
    assert_eq!(sessions("", MIN).len(), 0);
}

#[test]
fn single_completed_session_above_threshold() {
    let raw = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(hours(&s[0]), 8.0);
    assert_eq!(s[0].session_type, "IDLE");
}

#[test]
fn session_shorter_than_threshold_is_filtered_out() {
    let raw = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 00:30:00,IDLE_RESUME\n";
    // しきい値を60分(3600秒)にすると、30分のセッションは睡眠として記録されない。
    assert_eq!(sessions(raw, 3600).len(), 0);
}

#[test]
fn session_exactly_at_threshold_is_kept() {
    let raw = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 01:00:00,IDLE_RESUME\n";
    assert_eq!(sessions(raw, 3600).len(), 1);
}

#[test]
fn multiple_completed_sessions_all_counted() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 08:00:00,IDLE_RESUME
2024-01-02 01:00:00,IDLE_START
2024-01-02 09:00:00,IDLE_RESUME
";
    assert_eq!(sessions(raw, MIN).len(), 2);
}

#[test]
fn unrelated_lines_are_ignored() {
    let raw = "\
not,a,valid,line
2024-01-01 00:00:00,IDLE_START
garbage
2024-01-01 08:00:00,IDLE_RESUME
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(hours(&s[0]), 8.0);
}

// ── 末尾の未クローズIDLE_START（進行中セッション） ────────────────────────────
//
// 今回発見した実バグの回帰テスト群。以前は「未クローズのIDLE_STARTの直後に来る
// 最初のDEVICE_ONで閉じる」ヒューリスティックがあり、夜中に紛れ込む無関係な
// DEVICE_ON（タブレット画面が一瞬ついただけ等）を拾って、実際の睡眠時間よりも
// 大幅に短い偽のセッションを作ってしまっていた。修正後は、進行中セッションは
// 完了扱いにせず単純に無視する（暫定睡眠時間の表示はcurrent_sleep_start()側の
// 責務）。

#[test]
fn trailing_unclosed_idle_start_alone_is_not_a_session() {
    let raw = "2024-01-01 01:00:00,IDLE_START\n";
    assert_eq!(sessions(raw, MIN).len(), 0);
}

#[test]
fn trailing_unclosed_idle_start_with_one_spurious_device_on_is_not_fabricated() {
    let raw = "\
2024-01-01 01:00:00,IDLE_START
2024-01-01 01:37:00,DEVICE_ON
";
    // 修正前はここで「01:00〜01:37」という37分の偽セッションが記録されていた。
    assert_eq!(sessions(raw, MIN).len(), 0);
}

#[test]
fn real_world_regression_overnight_device_on_noise() {
    // 実際に発生したバグと同じ形（IDLE_START直後から30分おきくらいにDEVICE_ONが
    // 何度も入り、IDLE_RESUMEがまだ来ていない）を、架空の日付で再現する。
    // 修正前は最初のDEVICE_ON（開始からわずか37分後）でセッションが打ち切られ、
    // 60分しきい値未満として睡眠バーが消えていた。
    let raw = "\
2024-01-01 01:28:11,IDLE_START
2024-01-01 02:05:38,DEVICE_ON
2024-01-01 02:35:47,DEVICE_ON
2024-01-01 04:05:38,DEVICE_ON
2024-01-01 04:39:13,DEVICE_ON
2024-01-01 06:42:52,APP_USAGE_START
2024-01-01 06:42:55,DEVICE_ON
2024-01-01 06:47:19,APP_USAGE_END
2024-01-01 07:52:34,DEVICE_ON
";
    // IDLE_RESUMEがまだ無い＝進行中。完了セッションとしては0件が正しい
    // （暫定睡眠時間としての表示は別のcurrent_sleep_start()が担当する）。
    assert_eq!(sessions(raw, 3600).len(), 0);
}

#[test]
fn device_on_before_idle_start_does_not_retroactively_close_it() {
    // DEVICE_ONがIDLE_STARTより前にあっても無関係（探索対象は開始より後のみ）。
    let raw = "\
2024-01-01 00:00:00,DEVICE_ON
2024-01-01 01:00:00,IDLE_START
";
    assert_eq!(sessions(raw, MIN).len(), 0);
}

// ── DEVICE_ONは睡眠判定に一切影響しない ───────────────────────────────────────

#[test]
fn device_on_inside_a_completed_session_has_no_effect() {
    let with_device_on = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 03:00:00,DEVICE_ON
2024-01-01 08:00:00,IDLE_RESUME
";
    let without = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n";
    let a = sessions(with_device_on, MIN);
    let b = sessions(without, MIN);
    assert_eq!(a.len(), b.len());
    assert_eq!(hours(&a[0]), hours(&b[0]));
}

#[test]
fn many_device_on_events_scattered_do_not_split_or_shorten_session() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 01:00:00,DEVICE_ON
2024-01-01 02:00:00,DEVICE_ON
2024-01-01 03:00:00,DEVICE_ON
2024-01-01 04:00:00,DEVICE_ON
2024-01-01 08:00:00,IDLE_RESUME
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(hours(&s[0]), 8.0);
}

// ── OUT区間（外出）による分割 ─────────────────────────────────────────────────

#[test]
fn out_gap_splits_session_into_two_pieces() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,OUT_START
2024-01-01 02:30:00,OUT_END
2024-01-01 08:00:00,IDLE_RESUME
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 2);
    assert_eq!(hours(&s[0]), 2.0);   // 00:00〜02:00
    assert_eq!(hours(&s[1]), 5.5);   // 02:30〜08:00
}

#[test]
fn out_gap_piece_shorter_than_threshold_is_dropped() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 00:10:00,OUT_START
2024-01-01 00:20:00,OUT_END
2024-01-01 08:00:00,IDLE_RESUME
";
    // 分割後の最初の断片（00:00〜00:10 = 10分）は60分しきい値未満なので捨てられる。
    let s = sessions(raw, 3600);
    assert_eq!(s.len(), 1);
    // 00:20〜08:00 = 7h40m
    assert!((hours(&s[0]) - 7.6667).abs() < 0.01);
}

#[test]
fn unclosed_out_start_is_ignored_not_treated_as_gap() {
    // OUT_ENDが来ない場合、そのOUT_STARTはギャップとして扱われない
    // （閉じたペアだけを見る、というPass 1の原則どおり）。
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,OUT_START
2024-01-01 08:00:00,IDLE_RESUME
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(hours(&s[0]), 8.0);
}

// ── APP_USAGE区間（タブレット実利用）による分割 ───────────────────────────────

#[test]
fn app_usage_gap_splits_session_same_as_out() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,APP_USAGE_START
2024-01-01 02:30:00,APP_USAGE_END
2024-01-01 08:00:00,IDLE_RESUME
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 2);
    assert_eq!(hours(&s[0]), 2.0);
    assert_eq!(hours(&s[1]), 5.5);
}

#[test]
fn app_usage_shorter_than_60s_is_noise_and_ignored() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,APP_USAGE_START
2024-01-01 02:00:30,APP_USAGE_END
2024-01-01 08:00:00,IDLE_RESUME
";
    // 30秒の利用はMIN_APP_USAGE_SECS(60秒)未満なのでギャップとして扱われず、
    // セッションは分割されない。
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(hours(&s[0]), 8.0);
}

#[test]
fn app_usage_close_together_are_coalesced_into_one_gap() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,APP_USAGE_START
2024-01-01 02:00:40,APP_USAGE_END
2024-01-01 02:01:30,APP_USAGE_START
2024-01-01 02:02:00,APP_USAGE_END
2024-01-01 08:00:00,IDLE_RESUME
";
    // 個々は40秒・30秒で単独ならノイズ扱いだが、90秒しか離れておらず
    // (APP_USAGE_MERGE_GAP_SECS=120秒以内)、統合後は120秒(60秒以上)なので
    // 1つの有効なギャップとして扱われ、セッションが分割される。
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 2);
}

#[test]
fn out_and_app_usage_overlapping_do_not_double_count() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,OUT_START
2024-01-01 03:00:00,APP_USAGE_START
2024-01-01 03:30:00,APP_USAGE_END
2024-01-01 04:00:00,OUT_END
2024-01-01 08:00:00,IDLE_RESUME
";
    // OUT(02:00-04:00)とAPP_USAGE(03:00-03:30)は重なっている。重複部分が
    // 二重に切り出されて余計な断片が生まれないことを確認する（2分割のまま）。
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 2);
    assert_eq!(hours(&s[0]), 2.0); // 00:00-02:00
    assert_eq!(hours(&s[1]), 4.0); // 04:00-08:00
}

// ── 削除マーカー ──────────────────────────────────────────────────────────────

#[test]
fn session_deleted_marker_excludes_that_session() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 08:00:00,IDLE_RESUME
2024-01-01 00:00:00,SESSION_DELETED
";
    assert_eq!(sessions(raw, MIN).len(), 0);
}

#[test]
fn session_deleted_marker_only_excludes_matching_start() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 08:00:00,IDLE_RESUME
2024-01-02 00:00:00,IDLE_START
2024-01-02 08:00:00,IDLE_RESUME
2024-01-01 00:00:00,SESSION_DELETED
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].start, "2024-01-02 00:00:00");
}

// ── 手動セッション（sleep_manual.txt） ────────────────────────────────────────

#[test]
fn manual_session_is_included_with_manual_type() {
    let raw = "";
    let manual = "2024-01-01 13:00:00,2024-01-01 14:00:00\n";
    let s = sessions_with_manual(raw, manual, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].session_type, "MANUAL");
    assert_eq!(hours(&s[0]), 1.0);
}

#[test]
fn manual_deleted_marker_excludes_manual_session() {
    let raw = "";
    let manual = "\
2024-01-01 13:00:00,2024-01-01 14:00:00
2024-01-01 13:00:00,MANUAL_DELETED
";
    assert_eq!(sessions_with_manual(raw, manual, MIN).len(), 0);
}

#[test]
fn manual_and_auto_sessions_coexist() {
    let raw = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n";
    let manual = "2024-01-01 13:00:00,2024-01-01 14:00:00\n";
    let s = sessions_with_manual(raw, manual, MIN);
    assert_eq!(s.len(), 2);
}

// ── PC電源(SUSPEND/SHUTDOWN/RESUME/STARTUP) ───────────────────────────────────

#[test]
fn power_session_recorded_when_above_threshold() {
    let raw = "\
2024-01-01 00:00:00,SUSPEND
2024-01-01 08:00:00,RESUME
";
    let s = sessions(raw, 3600);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].session_type, "POWER");
    assert_eq!(hours(&s[0]), 8.0);
}

#[test]
fn power_session_below_threshold_not_recorded() {
    let raw = "\
2024-01-01 00:00:00,SHUTDOWN
2024-01-01 00:10:00,STARTUP
";
    assert_eq!(sessions(raw, 3600).len(), 0);
}

#[test]
fn power_loss_recorded_for_large_gap_without_pending_suspend() {
    // SUSPEND/SHUTDOWNの記録が無いまま、前のイベントから4時間以上空いてRESUMEが
    // 来た場合は、停電・強制終了などによる「POWER_LOSS」として記録する。
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 09:00:00,RESUME
";
    let s = sessions(raw, 3600);
    assert!(s.iter().any(|x| x.session_type == "POWER_LOSS"));
}

#[test]
fn power_loss_not_recorded_for_gap_under_four_hours() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 02:00:00,RESUME
";
    let s = sessions(raw, MIN);
    assert!(!s.iter().any(|x| x.session_type == "POWER_LOSS"));
}

// ── しきい値0のエッジケース ────────────────────────────────────────────────────

#[test]
fn zero_duration_session_with_zero_threshold_is_kept() {
    // duration >= min_sleep_secs は 0 >= 0 で true になるため、しきい値0のときは
    // 開始・終了が同時刻でもセッションとして残る（境界条件の確認）。
    let raw = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 00:00:00,IDLE_RESUME\n";
    assert_eq!(sessions(raw, 0).len(), 1);
}

#[test]
fn events_out_of_chronological_order_in_file_are_sorted_first() {
    // ファイル内の行順が前後していても、パース時にepochでソートされるため
    // 結果は正しい。
    let raw = "\
2024-01-01 08:00:00,IDLE_RESUME
2024-01-01 00:00:00,IDLE_START
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 1);
    assert_eq!(hours(&s[0]), 8.0);
}

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

// ── is_out_from_content ───────────────────────────────────────────────────────

#[test]
fn is_out_from_content_true_after_out_start() {
    assert!(is_out_from_content("2024-01-01 00:00:00,OUT_START\n"));
}

#[test]
fn is_out_from_content_false_after_out_end() {
    assert!(!is_out_from_content(
        "2024-01-01 00:00:00,OUT_START\n2024-01-01 01:00:00,OUT_END\n"
    ));
}

#[test]
fn is_out_from_content_device_on_also_cancels_out() {
    // DEVICE_ON / APP_USAGE_START は「在宅解除」専用として扱われるため、
    // 外出フラグを解除する（睡眠セッションの分割には使わないのと別軸の話）。
    assert!(!is_out_from_content(
        "2024-01-01 00:00:00,OUT_START\n2024-01-01 01:00:00,DEVICE_ON\n"
    ));
}

// ── coalesce_and_filter_app_usage ──────────────────────────────────────────────

#[test]
fn coalesce_filters_out_short_isolated_usage() {
    let pairs = vec![(0i64, "t0".to_string(), 30i64, "t1".to_string())];
    assert_eq!(coalesce_and_filter_app_usage(pairs).len(), 0);
}

#[test]
fn coalesce_merges_close_pairs_and_keeps_if_combined_long_enough() {
    let pairs = vec![
        (0i64, "a".to_string(), 40i64, "b".to_string()),
        (100i64, "c".to_string(), 130i64, "d".to_string()),
    ];
    // gap = 100-40 = 60秒 <= 120秒 → 統合。統合後の長さ = 130 = 60秒以上 → 採用。
    let merged = coalesce_and_filter_app_usage(pairs);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].0, 0);
    assert_eq!(merged[0].2, 130);
}

#[test]
fn coalesce_keeps_far_apart_pairs_separate() {
    let pairs = vec![
        (0i64, "a".to_string(), 90i64, "b".to_string()),
        (1000i64, "c".to_string(), 1090i64, "d".to_string()),
    ];
    assert_eq!(coalesce_and_filter_app_usage(pairs).len(), 2);
}

// ── 計測対象外の日（DAY_EXCLUDED / DAY_INCLUDED） ─────────────────────────────

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
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 08:00:00,IDLE_RESUME
2024-01-02 00:00:00,DAY_EXCLUDED:2024-01-01
";
    let s = sessions(raw, MIN);
    // バーとしては表示し続けるため、除外してもセッション自体は消えない。
    assert_eq!(s.len(), 1);
    assert!(s[0].excluded);
}

#[test]
fn session_on_non_excluded_day_is_not_flagged() {
    let raw = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n";
    let s = sessions(raw, MIN);
    assert!(!s[0].excluded);
}

#[test]
fn excluding_one_day_does_not_affect_another_day_session() {
    let raw = "\
2024-01-01 00:00:00,IDLE_START
2024-01-01 08:00:00,IDLE_RESUME
2024-01-02 00:00:00,IDLE_START
2024-01-02 08:00:00,IDLE_RESUME
2024-01-03 00:00:00,DAY_EXCLUDED:2024-01-01
";
    let s = sessions(raw, MIN);
    assert_eq!(s.len(), 2);
    let day1 = s.iter().find(|x| x.start.starts_with("2024-01-01")).unwrap();
    let day2 = s.iter().find(|x| x.start.starts_with("2024-01-02")).unwrap();
    assert!(day1.excluded);
    assert!(!day2.excluded);
}

#[test]
fn manual_session_on_excluded_day_is_flagged() {
    let raw = "2024-01-01 00:00:00,DAY_EXCLUDED:2024-01-01\n";
    let manual = "2024-01-01 13:00:00,2024-01-01 14:00:00\n";
    let s = sessions_with_manual(raw, manual, MIN);
    assert_eq!(s.len(), 1);
    assert!(s[0].excluded);
}

// ── 睡眠判定に使うアプリ（USAGE_APP_SEEN/ALLOWED/DENIED） ──────────────────────

#[test]
fn usage_package_seen_only_uses_default_allowed_state() {
    let raw = "\
2024-01-01 00:00:00,USAGE_APP_SEEN:com.android.chrome|Chrome
2024-01-01 00:00:01,USAGE_APP_SEEN:com.sleeptracker.app|睡眠トラッカー
";
    let list = usage_packages_from_content(raw);
    let chrome = list.iter().find(|e| e.package == "com.android.chrome").unwrap();
    let self_pkg = list.iter().find(|e| e.package == "com.sleeptracker.app").unwrap();
    assert!(chrome.allowed, "既知の除外対象でないアプリは既定でON");
    assert!(!self_pkg.allowed, "自アプリ自身は既定でOFF");
    assert_eq!(chrome.label, "Chrome");
    assert_eq!(self_pkg.label, "睡眠トラッカー");
}

#[test]
fn usage_package_denied_marker_overrides_default_allow() {
    let raw = "\
2024-01-01 00:00:00,USAGE_APP_SEEN:com.android.chrome|Chrome
2024-01-01 00:00:01,USAGE_APP_DENIED:com.android.chrome
";
    let list = usage_packages_from_content(raw);
    let chrome = list.iter().find(|e| e.package == "com.android.chrome").unwrap();
    assert!(!chrome.allowed);
}

#[test]
fn usage_package_allowed_marker_overrides_default_deny() {
    let raw = "\
2024-01-01 00:00:00,USAGE_APP_SEEN:com.miui.home|ホーム
2024-01-01 00:00:01,USAGE_APP_ALLOWED:com.miui.home
";
    let list = usage_packages_from_content(raw);
    let home = list.iter().find(|e| e.package == "com.miui.home").unwrap();
    assert!(home.allowed, "既定OFFでも明示的にALLOWEDされていればON");
}

#[test]
fn usage_package_last_marker_wins_regardless_of_file_order() {
    // ファイル内の記述順ではなく、タイムスタンプの新しい方が勝つ。
    let raw = "\
2024-01-01 10:00:00,USAGE_APP_DENIED:com.android.chrome
2024-01-01 09:00:00,USAGE_APP_ALLOWED:com.android.chrome
";
    let list = usage_packages_from_content(raw);
    let chrome = list.iter().find(|e| e.package == "com.android.chrome").unwrap();
    assert!(!chrome.allowed, "10:00のDENIEDが09:00のALLOWEDより新しいので勝つ");
}
