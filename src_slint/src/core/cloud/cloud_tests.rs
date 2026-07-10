//! cloud_tests.rs — cloud.rs のテスト専用サブモジュール
//!
//! 役割 : merge_into_local（Drive⇔ローカルのマージ）、世代番号ベースの
//!        merge_or_adopt_atによる上書き系操作の伝播ロジック、および
//!        looks_like_events_content等のクライアント側内容検証の単体テスト。
//!        `#[cfg(test)]` のみでビルドされる。
//!
//! 依存 : super::generation::{merge_into_local, merge_or_adopt_at},
//!        super::validation::{looks_like_html_or_js, looks_like_events_content}

use super::generation::{merge_into_local, merge_or_adopt_at, read_generation_at, write_generation_at};
use super::validation::{looks_like_html_or_js, looks_like_events_content};

// 実データと衝突しないよう、プロセスIDとテスト名からユニークな一時ファイルパスを作る。
fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("sleep_tracker_cloud_test_{}_{}.txt", std::process::id(), name));
    p
}

#[test]
fn merge_into_local_plain_union_without_reset_marker() {
    let path = temp_path("plain_union");
    std::fs::write(&path, "2024-01-01 00:00:00,IDLE_START\n").unwrap();
    let changed = merge_into_local(&path, "2024-01-01 08:00:00,IDLE_RESUME\n");
    assert!(changed);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("IDLE_START"));
    assert!(content.contains("IDLE_RESUME"));
    let _ = std::fs::remove_file(&path);
}

// 回帰テスト: 「全データ削除」でクラウドが空になった後に「バックアップから復元」で
// ローカルへ古いタイムスタンプの実データを書き戻すと、以前のHARD_RESETマーカー
// フィルタがこれを「リセット前の残骸」と誤認して丸ごと破棄してしまうバグがあった。
// merge_into_localは行の新旧に関わらず常に単純なunion（和集合）でなければならない
// （全体リセットの伝播は世代番号側のガードが担う。merge_or_adopt_at参照）。
#[test]
fn merge_into_local_keeps_old_timestamped_local_data_against_empty_drive() {
    let path = temp_path("restore_old_data_survives");
    std::fs::write(&path, "2020-01-01 00:00:00,IDLE_START\n2020-01-01 08:00:00,IDLE_RESUME\n").unwrap();
    let changed = merge_into_local(&path, "");
    assert!(!changed, "ローカルのみで変化がない場合はfalseを返すべき");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("2020-01-01 00:00:00,IDLE_START"), "古いタイムスタンプの復元データが破棄されてはいけない");
    let _ = std::fs::remove_file(&path);
}

// HARD_RESETという単語を含むタグの行が万一残っていても、もはや特別扱いせず
// 通常のイベント行として単純にunionされること。
#[test]
fn merge_into_local_without_reset_keeps_all_local_lines() {
    let path = temp_path("no_reset_keeps_all");
    std::fs::write(&path, "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n").unwrap();
    let drive_content = "2024-01-02 00:00:00,IDLE_START\n2024-01-02 08:00:00,IDLE_RESUME\n";
    merge_into_local(&path, drive_content);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("2024-01-01 00:00:00,IDLE_START"));
    assert!(content.contains("2024-01-02 00:00:00,IDLE_START"));
    let _ = std::fs::remove_file(&path);
}

// クラウドの世代がローカルより新しければ、ローカル未同期分を丸ごと破棄して
// Drive内容を採用しなければならない（2端末がオフライン気味の状態でほぼ同時に
// 「全データ削除」した場合の対策）。
#[test]
fn merge_or_adopt_discards_local_when_cloud_generation_ahead() {
    let path = temp_path("gen_adopt_data");
    let gen_path = temp_path("gen_adopt_gen");
    std::fs::write(&path, "2024-01-05 00:00:00,IDLE_START\n").unwrap();
    write_generation_at(&gen_path, 1);
    let drive_content = "";
    let changed = merge_or_adopt_at(&path, drive_content, Some(2), &gen_path);
    assert!(changed);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("IDLE_START"), "古い世代のローカル専用行が復活してはいけない");
    assert_eq!(read_generation_at(&gen_path), 2, "ローカルの世代番号がクラウドに追従すること");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&gen_path);
}

// 世代が同じ（=リセットは起きていない）なら、これまで通り通常のunionマージに任せること。
#[test]
fn merge_or_adopt_uses_normal_merge_when_generation_unchanged() {
    let path = temp_path("gen_same_data");
    let gen_path = temp_path("gen_same_gen");
    std::fs::write(&path, "2024-01-05 00:00:00,IDLE_START\n").unwrap();
    write_generation_at(&gen_path, 3);
    let drive_content = "2024-01-06 00:00:00,IDLE_START\n";
    merge_or_adopt_at(&path, drive_content, Some(3), &gen_path);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("2024-01-05 00:00:00,IDLE_START"), "世代が同じならローカル専用行は残るべき");
    assert!(content.contains("2024-01-06 00:00:00,IDLE_START"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&gen_path);
}

// 世代番号の取得に失敗した場合（オフライン等）は、世代ゲートを素通りさせて
// 通常のunionマージ(merge_into_local)に任せること。
#[test]
fn merge_or_adopt_falls_back_to_normal_merge_when_generation_unavailable() {
    let path = temp_path("gen_none_data");
    let gen_path = temp_path("gen_none_gen");
    std::fs::write(&path, "2024-01-05 00:00:00,IDLE_START\n").unwrap();
    write_generation_at(&gen_path, 5);
    let drive_content = "2024-01-06 00:00:00,IDLE_START\n";
    merge_or_adopt_at(&path, drive_content, None, &gen_path);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("2024-01-05 00:00:00,IDLE_START"));
    assert!(content.contains("2024-01-06 00:00:00,IDLE_START"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&gen_path);
}

// 回帰テスト: GASのスクリプトロジックを経由せず（Googleの認証リダイレクト等で）
// ログインページのHTMLがそのまま返ってくる事故が実際にあった。GAS側の"error:"検知
// をすり抜けるケースに備え、クライアント側でも独立に中身を検証できること。
#[test]
fn looks_like_html_or_js_detects_login_page() {
    let html = "<!doctype html><html><head><script>document.querySelector('a')</script></head></html>";
    assert!(looks_like_html_or_js(html));
}

#[test]
fn looks_like_html_or_js_accepts_normal_events_content() {
    let content = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n";
    assert!(!looks_like_html_or_js(content));
}

#[test]
fn looks_like_events_content_accepts_real_event_lines() {
    let content = "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n2024-01-02 00:00:00,USAGE_APP_SEEN:com.android.settings|設定\n";
    assert!(looks_like_events_content(content));
}

#[test]
fn looks_like_events_content_rejects_html() {
    let html = "<!doctype html><html><head><title>Sign in</title></head><body></body></html>";
    assert!(!looks_like_events_content(html));
}

#[test]
fn looks_like_events_content_rejects_mostly_malformed_lines() {
    let content = "not a valid line\nanother bad line\nyet another\n";
    assert!(!looks_like_events_content(content));
}

#[test]
fn looks_like_events_content_tolerates_a_few_malformed_lines() {
    // 90%以上が正しい形式なら許容する
    let mut content = String::new();
    for _ in 0..20 {
        content.push_str("2024-01-01 00:00:00,IDLE_START\n");
    }
    content.push_str("stray line without timestamp\n");
    assert!(looks_like_events_content(&content));
}
