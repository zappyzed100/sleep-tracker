//! cloud_tests.rs — cloud.rs のテスト専用サブモジュール
//!
//! 役割 : merge_into_local（Drive⇔ローカルのマージ）とHARD_RESETマーカー
//!        （events::HARD_RESET_TAG）、および世代番号ベースのmerge_or_adopt_atによる
//!        上書き系操作の伝播ロジックの単体テスト。`#[cfg(test)]` のみでビルドされる。
//!
//! 依存 : なし（`super::*` で親モジュール cloud.rs の非公開関数を参照するのみ）

use super::*;

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

// 全データ削除・データ圧縮のような上書き系操作がもう一方の端末で実行されると、
// 実行時刻を持つHARD_RESET行がDrive経由で送られてくる。通常のunionマージだと
// ローカルにしか無い古い行が復活してしまうため、マーカー時刻以前のローカル専用行は
// 破棄されなければならない。
#[test]
fn merge_into_local_hard_reset_discards_old_local_only_lines() {
    let path = temp_path("hard_reset_discard");
    std::fs::write(&path, "2024-01-01 00:00:00,IDLE_START\n2024-01-01 08:00:00,IDLE_RESUME\n").unwrap();
    let drive_content = "2024-01-02 00:00:00,HARD_RESET\n";
    let changed = merge_into_local(&path, drive_content);
    assert!(changed);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("IDLE_START"), "reset前のローカル専用行が復活してはいけない");
    assert!(content.contains("HARD_RESET"));
    let _ = std::fs::remove_file(&path);
}

// reset後にこの端末が新しく記録したローカル専用行は、破棄されず残らなければならない。
#[test]
fn merge_into_local_hard_reset_keeps_local_lines_after_reset_timestamp() {
    let path = temp_path("hard_reset_keep_new");
    std::fs::write(&path, "2024-01-03 00:00:00,IDLE_START\n2024-01-03 08:00:00,IDLE_RESUME\n").unwrap();
    let drive_content = "2024-01-02 00:00:00,HARD_RESET\n";
    merge_into_local(&path, drive_content);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("2024-01-03 00:00:00,IDLE_START"), "reset後のローカル専用行は残るべき");
    assert!(content.contains("HARD_RESET"));
    let _ = std::fs::remove_file(&path);
}

// 複数のHARD_RESETマーカーがある場合は最新（最大タイムスタンプ）を基準にする。
#[test]
fn merge_into_local_hard_reset_uses_latest_marker_when_multiple() {
    let path = temp_path("hard_reset_latest");
    std::fs::write(&path, "2024-01-01 00:00:00,IDLE_START\n2024-01-05 00:00:00,IDLE_START\n").unwrap();
    let drive_content = "2024-01-02 00:00:00,HARD_RESET\n2024-01-04 00:00:00,HARD_RESET\n";
    merge_into_local(&path, drive_content);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("2024-01-01 00:00:00,IDLE_START"));
    assert!(content.contains("2024-01-05 00:00:00,IDLE_START"));
    let _ = std::fs::remove_file(&path);
}

// HARD_RESETが無ければ、これまで通り単純なunionマージのままであること。
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

// クラウドの世代がローカルより新しければ、HARD_RESETマーカーが無くても
// ローカル未同期分を丸ごと破棄してDrive内容を採用しなければならない
// （2端末がオフライン気味の状態でほぼ同時に「全データ削除」した場合の対策）。
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
// HARD_RESETマーカーベースの通常マージに任せること。
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
