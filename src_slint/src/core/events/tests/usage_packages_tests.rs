// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// usage_packages_tests.rs — 睡眠判定に使うアプリ（USAGE_APP_SEEN/ALLOWED/DENIED）のテスト
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::core::events::usage_packages::usage_packages_from_content;

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

// 一覧が長くなっても、チェック済み（睡眠判定に使う）アプリが表示件数制限の
// 下の方に埋もれないよう、チェック済みを先頭に配置する。
#[test]
fn usage_packages_sorted_with_allowed_first_then_by_label() {
    let raw = "\
2024-01-01 00:00:00,USAGE_APP_SEEN:com.b|Bravo
2024-01-01 00:00:01,USAGE_APP_SEEN:com.a|Alpha
2024-01-01 00:00:02,USAGE_APP_SEEN:com.z|Zulu
2024-01-01 00:00:03,USAGE_APP_DENIED:com.a
2024-01-01 00:00:04,USAGE_APP_DENIED:com.z
";
    let list = usage_packages_from_content(raw);
    let labels: Vec<&str> = list.iter().map(|e| e.label.as_str()).collect();
    // Bravoだけallowed（既定でON）、Alpha/Zuluは明示的にDENIED。
    assert_eq!(labels, vec!["Bravo", "Alpha", "Zulu"]);
}
