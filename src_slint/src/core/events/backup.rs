//! backup.rs — バックアップ・復元・圧縮などデータライフサイクル管理
//!
//! 役割 : sleep_events.txt全体の取得・復元・全削除、日次ローカル自動バックアップ、
//!        バックアップ一覧、進行中セッションの検出、データ圧縮(compact_data)を担当する。
//!
//! 依存 : super::{TAG, SESSION_CACHE, EVENTS_FILE_LOCK},
//!        super::parsing::{sort_events_file, parse_sessions_rust}

use super::{TAG, SESSION_CACHE, EVENTS_FILE_LOCK};
use super::parsing::{sort_events_file, parse_sessions_rust};

pub fn get_events_content() -> Result<String, String> {
    let path = crate::data_dir().join("sleep_events.txt");
    if !path.exists() { return Ok(String::new()); }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

pub fn restore_events(content: String) -> Result<(), String> {
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    sort_events_file(&path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    Ok(())
}

// ── 日次ローカル自動バックアップ（世代は自動削除しない） ─────────────────────
//
// 手動の「バックアップ」ボタン（設定タブ）と同じ内容をファイルダイアログなしで
// crate::backups_base_dir()/backups/ に書き出す（PCはdata/backups/、Androidは
// ファイルマネージャーから参照できる外部ストレージ領域のbackups/）。
// 前回バックアップ時刻は data/last_auto_backup.txt に保持し、アプリ再起動を
// またいでも正しく約1日おきになるようにする。
// 1件あたり数KB程度なので自動削除はせず、貯まった分は「バックアップを削除」
// ボタン（clear_backups）で手動削除する。
// Google Driveへの自動バックアップ（cloud::auto_backup_after_event）とは独立した、
// ローカルディスク上の世代バックアップ。
// PC版はmonitor.rsの常駐ループから毎時呼ばれる。Android版はプロセス常駐の
// バックグラウンドスレッドを持てないため、platform/android/bg.rsのフォアグラウンド中
// タイマーから呼ばれる（＝アプリを開いた時だけ判定される）。

const BACKUP_INTERVAL_DAYS: i64 = 1;

pub fn maybe_auto_backup(data_dir: &std::path::Path) {
    use chrono::{Duration as CDur, Local};

    let marker_path = data_dir.join("last_auto_backup.txt");

    let due = match std::fs::read_to_string(&marker_path) {
        Ok(s) => match chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S") {
            Ok(last) => (Local::now().naive_local() - last) >= CDur::days(BACKUP_INTERVAL_DAYS),
            Err(_) => true,
        },
        // マーカーがない（初回起動）場合は、以降1日おきの基準点を作るため即バックアップする。
        Err(_) => true,
    };
    if !due {
        return;
    }

    let backups_dir = crate::backups_base_dir().join("backups");
    if std::fs::create_dir_all(&backups_dir).is_err() {
        eprintln!("{} auto_backup: ERROR backups/ ディレクトリを作成できません", TAG);
        return;
    }

    let date = Local::now().format("%Y-%m-%d");
    let mut wrote_any = false;
    for name in ["sleep_events.txt", "sleep_manual.txt"] {
        let src = data_dir.join(name);
        let Ok(content) = std::fs::read_to_string(&src) else { continue };
        let dest = backups_dir.join(format!("{}_{}", date, name));
        match std::fs::write(&dest, &content) {
            Ok(()) => {
                wrote_any = true;
                eprintln!("{} auto_backup: {:.1}KB → {:?}", TAG, content.len() as f64 / 1024.0, dest);
            }
            Err(e) => eprintln!("{} auto_backup: ERROR {}: {}", TAG, name, e),
        }
    }

    if wrote_any {
        let _ = std::fs::write(&marker_path, Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
    }
}

// crate::backups_base_dir()/backups/ 以下を全削除する（手動バックアップ・自動バックアップとも対象）。
// 現在のsleep_events.txt/sleep_manual.txt自体には触れない。
pub fn clear_backups(backups_base: &std::path::Path) -> Result<(), String> {
    let backups_dir = backups_base.join("backups");
    if !backups_dir.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(&backups_dir).map_err(|e| e.to_string())?;
    for entry in entries.filter_map(|e| e.ok()) {
        if entry.path().is_file() {
            std::fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize, Clone)]
pub struct BackupEntry {
    pub path: String,
    pub label: String,
}

// crate::backups_base_dir()/backups/ 内のファイルを更新日時が新しい順に列挙する。
// OSのファイルピッカーはソート順を指定できない（PCのExplorer・AndroidのSAFとも
// アプリ側から強制する手段がない）ため、アプリ内の一覧表示で確実に新しい順を保証する。
// 手動バックアップ・自動バックアップの両方がこのフォルダに保存されるため両方を含む。
pub fn list_backups() -> Vec<BackupEntry> {
    let backups_dir = crate::backups_base_dir().join("backups");
    let Ok(entries) = std::fs::read_dir(&backups_dir) else { return Vec::new() };

    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));

    files.into_iter()
        .map(|(_, path)| {
            let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            BackupEntry { path: path.to_string_lossy().to_string(), label }
        })
        .collect()
}

// ローカルの sleep_events.txt / sleep_manual.txt を両方とも空にする。
// クラウド（Drive・スプレッドシート）は消さない。
// もう一方の端末への伝播はcloud::clear_cloud_data_and_push_reset側の
// 世代番号（GAS側のGENERATION、cloud.rs参照）が担う。
pub fn clear_all_data() -> Result<(), String> {
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, "").map_err(|e| e.to_string())?;
    let manual_path = crate::data_dir().join("sleep_manual.txt");
    std::fs::write(&manual_path, "").map_err(|e| e.to_string())?;
    *SESSION_CACHE.lock().unwrap() = None;
    Ok(())
}

// ファイルを時刻順に走査し、START系イベントで開き、対応するEND系イベントで閉じる。
// 現在進行中で閉じていない IDLE_START / OUT_START のタイムスタンプを検出する
// （compact_data と current_sleep_start の両方から使う共通ロジック）。
pub(super) fn detect_open_idle_and_out(raw: &str) -> (Option<String>, Option<String>) {
    let mut open_idle: Option<String> = None;
    let mut open_out: Option<String> = None;
    for line in raw.lines() {
        let line = line.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}');
        let Some(c) = line.find(',') else { continue };
        let ts = &line[..c];
        match &line[c + 1..] {
            "IDLE_START" => open_idle = Some(ts.to_string()),
            "IDLE_RESUME" => open_idle = None,
            "OUT_START"  => open_out = Some(ts.to_string()),
            "OUT_END" | "IN_HOUSE" => open_out = None,
            _ => {}
        }
    }
    (open_idle, open_out)
}

// 現在進行中（まだ IDLE_RESUME が来ていない）の睡眠セッションの開始時刻を返す。
// 暫定睡眠時間の表示用（タブレットで寝ている最中に一瞬起きて確認する用途）。
// 外出中(OUT_START)にIDLE_STARTが来た場合は「起きている」ままなので None を返す
// （現状のセッション判定ロジックと同じ考え方：外出中はPC放置を睡眠とみなさない）。
pub fn current_sleep_start() -> Option<String> {
    let events_path = crate::data_dir().join("sleep_events.txt");
    let raw = std::fs::read_to_string(&events_path).ok()?;
    let (open_idle, open_out) = detect_open_idle_and_out(&raw);
    if open_out.is_some() {
        return None;
    }
    open_idle
}

// セッション（IDLE_START/IDLE_RESUME等）ではなく、日単位・アプリ単位の設定や
// 履歴を表す「メタデータ行」のタグ接頭辞。compact_data はセッションだけを
// 残して作り直すため、ここに載っていないタグの行は圧縮のたびに失われる。
// 新しい種類のマーカーを追加する時はここに足すだけでよい。
const PRESERVED_METADATA_TAG_PREFIXES: &[&str] = &[
    "DAY_EXCLUDED:", "DAY_INCLUDED:",
];

// 生の内容からPRESERVED_METADATA_TAG_PREFIXESに該当する行だけを抜き出す
// （compact_data参照）。
pub(super) fn extract_preserved_metadata_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}'))
        .filter(|l| {
            let Some((_, tag)) = l.split_once(',') else { return false };
            PRESERVED_METADATA_TAG_PREFIXES.iter().any(|p| tag.starts_with(p))
        })
        .map(|l| l.to_string())
        .collect()
}

// sleep_events.txt / sleep_manual.txt を、実際にセッションとしてパースされている
// 内容だけの最小構成に作り直す（不要な生イベント・削除済みマーカーなどを一掃する）。
// 手順：一度セッションリストを構築し、それをIDLE_START/IDLE_RESUMEペアとして再構築する
// （sleep_manual.txtの内容もここに統合されるため、sleep_manual.txt自体は空にする）。
// 現在進行中で閉じていないIDLE_START・OUT_STARTがあれば、それだけは生イベントとして
// そのまま残す（削除すると進行中の睡眠/外出状態を見失うため）。
// 戻り値: 再構築後のsleep_events.txtの内容（Driveへの直接pushに再利用するため）。
pub fn compact_data() -> Result<String, String> {
    let events_path = crate::data_dir().join("sleep_events.txt");
    let manual_path = crate::data_dir().join("sleep_manual.txt");

    let sessions = parse_sessions_rust()?;

    let raw = std::fs::read_to_string(&events_path).unwrap_or_default();
    let (open_idle, open_out) = detect_open_idle_and_out(&raw);

    // 各日の計測対象外設定（DAY_EXCLUDED/DAY_INCLUDED）はセッションではないため
    // 圧縮対象外とし、そのまま引き継ぐ（消すとユーザーが設定した計測対象外設定が
    // 圧縮のたびに失われてしまうため）。
    let metadata_lines = extract_preserved_metadata_lines(&raw);

    let mut lines: Vec<String> = Vec::with_capacity(sessions.len() * 2 + 2 + metadata_lines.len());
    for s in &sessions {
        lines.push(format!("{},IDLE_START", s.start));
        lines.push(format!("{},IDLE_RESUME", s.end));
    }
    if let Some(ts) = open_idle {
        lines.push(format!("{},IDLE_START", ts));
    }
    if let Some(ts) = open_out {
        lines.push(format!("{},OUT_START", ts));
    }
    lines.extend(metadata_lines);
    lines.sort();
    let content = lines.join("\n") + "\n";
    // 圧縮でsleep_manual.txtの内容はsleep_events.txt側に統合済みのため空にする。
    // もう一方の端末への伝播（圧縮で消えた生イベントが復活しないようにする）は
    // cloud::push_authoritative_content_to_driveの世代番号ガードが担う。
    let manual_content = String::new();

    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    std::fs::write(&events_path, &content).map_err(|e| e.to_string())?;
    std::fs::write(&manual_path, &manual_content).map_err(|e| e.to_string())?;
    drop(_lock);

    *SESSION_CACHE.lock().unwrap() = None;
    eprintln!("{} compact_data: {} sessions → {} lines ({:.1}KB)", TAG, sessions.len(), lines.len(), content.len() as f64 / 1024.0);
    Ok(content)
}
