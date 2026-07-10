//! generation.rs — 世代番号ガード・Drive⇔ローカルのunionマージ
//!
//! 役割 : クラウドの「世代番号」（GAS側でLockServiceにより排他的に払い出される、
//!        全削除・圧縮のたびに1つ進むカウンタ）による全体リセット系操作の伝播判定と、
//!        通常のイベント追記どうしをunion（和集合）でマージするmerge_into_localを担当する。
//!        以前併用していた行単位・時刻ベースのHARD_RESETマーカーは、時計のズレでの
//!        誤判定に加え、世代が一致している状態でも「復元」等が書き込む古い
//!        タイムスタンプのデータを誤って破棄する副作用があったため廃止した。
//!
//! 依存 : super::TAG, crate::core::events::EVENTS_FILE_LOCK

use std::sync::atomic::Ordering;

use super::TAG;
use crate::core::events::EVENTS_FILE_LOCK;

fn local_generation_path() -> std::path::PathBuf {
    crate::data_dir().join("generation.txt")
}

// パスを引数に取る形にして、テストが実データのgeneration.txtに触れず
// 一時ファイルで検証できるようにする（merge_into_localの`path`引数と同じ考え方）。
pub(super) fn read_generation_at(path: &std::path::Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub(super) fn write_generation_at(path: &std::path::Path, gen: u64) {
    let _ = std::fs::write(path, gen.to_string());
}

pub(super) fn save_local_generation(gen: u64) { write_generation_at(&local_generation_path(), gen) }

// クラウドの現在の世代番号を取得する。取得失敗時（オフライン等）はNoneを返し、
// 呼び出し側は世代ゲートを素通りさせて通常のunionマージ(merge_into_local)に任せる。
pub(super) fn fetch_cloud_generation(base_url: &str, secret: &str) -> Option<u64> {
    let url = format!("{}?secret={}&action=get_generation", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    resp.text().ok()?.trim().parse().ok()
}

// クラウドの世代がローカルより新しければ、Driveの内容を通常マージせず丸ごと
// 採用する（この端末が「全削除/圧縮」を知らない間にもう一方の端末がそれを
// 実行した場合、ローカル未同期分を破棄してクラウド側を正とする）。世代が
// 同じ・取得失敗時は、通常のunionマージ(merge_into_local)に任せる。
pub(super) fn merge_or_adopt_at(path: &std::path::Path, drive_content: &str, cloud_gen: Option<u64>, gen_path: &std::path::Path) -> bool {
    if let Some(cg) = cloud_gen {
        let lg = read_generation_at(gen_path);
        if cg > lg {
            eprintln!("{} merge_or_adopt: cloud generation ahead (local={} cloud={}) — adopting Drive content wholesale ({:?})", TAG, lg, cg, path.file_name());
            let _ = std::fs::write(path, drive_content);
            write_generation_at(gen_path, cg);
            return true;
        }
    }
    merge_into_local(path, drive_content)
}

pub(super) fn merge_or_adopt(path: &std::path::Path, drive_content: &str, cloud_gen: Option<u64>) -> bool {
    merge_or_adopt_at(path, drive_content, cloud_gen, &local_generation_path())
}

// push直前にクラウドの世代が変わっていないか再確認する。変わっていれば、
// pull時に見た内容を基にした今回のマージ結果は既に古くなっている可能性があるため
// pushを見送る（false）。次の同期サイクルで新しい世代を検知し、やり直しがきく。
pub(super) fn generation_unchanged_since(base_url: &str, secret: &str, observed_gen: Option<u64>) -> bool {
    match (observed_gen, fetch_cloud_generation(base_url, secret)) {
        (Some(old), Some(now)) if now > old => {
            eprintln!("{} generation_unchanged_since: cloud generation advanced ({} → {}) mid-sync — skip push this cycle", TAG, old, now);
            false
        }
        _ => true,
    }
}

// Merge drive_content lines into the local file (sort by timestamp, dedup).
// Returns true if the local file was updated (new lines added from Drive).
pub(super) fn merge_into_local(path: &std::path::Path, drive_content: &str) -> bool {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    // EVENTS_FILE_LOCK を取得して、sort_events_file / apply_mobile_event_line との
    // 競合によるデータ消失を防ぐ（merge中の並行append/sortをブロックする）
    let _lock = EVENTS_FILE_LOCK.lock().unwrap();

    let local_raw = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else { String::new() };
    // Strip UTF-8 BOM (U+FEFF) that PowerShell/Windows tools sometimes prepend.
    let local = local_raw.trim_start_matches('\u{FEFF}');
    let drive_content = drive_content.trim_start_matches('\u{FEFF}');

    // 処理済みのローカル行（BOM除去・trim・空行フィルタ済み）を先に作成
    let local_processed: Vec<String> = local.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let local_n = local_processed.len();
    let drive_n = drive_content.lines().filter(|l| !l.trim().is_empty()).count();
    eprintln!("{} merge_into_local #{}: local={} lines  drive={} lines", TAG, n, local_n, drive_n);

    let drive_processed: Vec<String> = drive_content.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}').to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // 全体リセット系操作（全データ削除・データ圧縮）を見逃していないかは
    // merge_or_adopt/fetch_cloud_generationの世代番号ガードが担うため、ここは
    // 純粋なunion（和集合）マージに徹する。
    let mut all: Vec<String> = local_processed.iter()
        .chain(drive_processed.iter())
        .cloned()
        .collect();
    // Sort by full content so dedup removes ALL duplicates (same-timestamp
    // alternating pairs like IN_HOUSE/STARTUP would survive timestamp-only sort).
    all.sort();
    all.dedup();
    // Re-sort by timestamp for chronological order.
    all.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));

    // Write if content changed (new lines added OR duplicates removed from local)
    let local_normalized = local_processed.join("\n") + "\n";
    let merged = all.join("\n") + "\n";
    if merged == local_normalized && all.len() == local_n {
        eprintln!("{} merge_into_local #{}: no change ({}  lines)", TAG, n, all.len());
        return false;
    }

    let delta = all.len() as i64 - local_n as i64;
    eprintln!("{} merge_into_local #{}: {} → {} lines ({:+})", TAG, n, local_n, all.len(), delta);
    if delta > 0 {
        // local_processed（trim済み）と比較して正確に追加行を特定
        let added_lines: Vec<&str> = all.iter()
            .filter(|l| !local_processed.iter().any(|lp| lp == l.as_str()))
            .map(|s| s.as_str()).collect();
        for (i, line) in added_lines.iter().enumerate().take(10) {
            eprintln!("{} merge_into_local #{}:   +[{}] {}", TAG, n, i, line);
        }
        if added_lines.len() > 10 {
            eprintln!("{} merge_into_local #{}: ... ({} more)", TAG, n, added_lines.len() - 10);
        }
    }

    let _ = std::fs::write(path, merged);
    true
}
