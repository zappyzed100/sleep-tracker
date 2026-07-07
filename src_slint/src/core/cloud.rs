//! cloud.rs — モバイルイベント取得・Drive バックアップ・クラウド同期
//!
//! 役割 : Google Apps Script 経由でモバイルデバイスのイベントを取得し
//!        sleep_events.txt に書き込む。Drive へのバックアップも担当する。
//!        Tauri版 src-tauri/src/cloud.rs の移植。
//!        `#[tauri::command] async fn` + `spawn_blocking` は同期関数に変更し、
//!        UIスレッドをブロックしないための非同期呼び出しは呼び出し側（main.rs）の
//!        std::thread::spawn に任せる。
//!        全体リセット系操作（全データ削除・データ圧縮）の伝播は、GAS側で
//!        LockServiceにより排他的に払い出す「世代番号」（worker/appsscript.gs参照）
//!        でガードする。以前は行単位・時刻ベースのHARD_RESETマーカーも併用していたが、
//!        時計のズレでの誤判定に加え、世代が一致している（＝リセットを見逃していない）
//!        状態でも「復元」等が書き込む古いタイムスタンプのデータを誤って破棄する副作用が
//!        あったため廃止した（世代番号だけで十分にカバーできるため。詳細は
//!        merge_or_adopt/generation_unchanged_sinceのコメント参照）。
//!        世代番号は全削除・圧縮のような一括リセットしか検知できず、通常の
//!        イベント追記どうしの競合（pull〜push間に別端末が割り込んで上書きし、
//!        マージされずに消えるロスト・アップデート）は検知できない。そのため
//!        pushのたびにpull直後の内容のSHA-256を送り、GAS側の実際の内容と
//!        食い違っていれば拒否する楽観的並行性制御を別途行う
//!        （sha256_hex/backup_to_drive_checked、worker/appsscript.gsの
//!        「G. 内容ハッシュ」参照）。
//!
//! 依存 : crate::data_dir, crate::http_client, config::load_config_inner,
//!        events::apply_mobile_event_line, events::sort_events_file,
//!        events::SESSION_CACHE, events::SessionCache, events::parse_sessions_rust
//! 公開 : `pull_mobile_events_inner`, `fetch_from_cloud`,
//!        `sync_gist`, `ensure_events_from_drive`, `test_mobile_connection`,
//!        `clear_cloud_data`, `clear_cloud_data_and_push_reset`, `push_authoritative_content_to_drive`,
//!        `is_sync_paused`, `set_sync_paused`

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::events::{
    SESSION_CACHE, SessionCache, parse_sessions_rust,
    apply_mobile_event_line, sort_events_file, sort_manual_file,
    EVENTS_FILE_LOCK,
};
use super::config::load_config_inner;

const TAG: &str = "[cloud]";

// pull直後の内容のSHA-256（16進文字列）。楽観的並行性制御のexpected_hashに使う
// （GAS側でも同じアルゴリズムで計算して比較する。worker/appsscript.gsのcomputeHash_参照）。
fn sha256_hex(content: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, content.as_bytes());
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

static CONSECUTIVE_ERRORS: AtomicU64 = AtomicU64::new(0);
// Prevents concurrent sync_mobile_inner calls (startup vs manual button press).
static SYNC_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// 自動同期の一時停止フラグ（data_dir/sync_paused ファイルの有無で永続化する）。
// 「同期を停止するボタン」用。手動の「今すぐ同期」ボタン・接続テスト・
// クラウド全削除などの明示的な操作は、このフラグの影響を受けない
// （ユーザーが明示的に押した操作は常に実行されるべきため）。
// 起動時の同期・定期同期・PC側のIDLE_START/RESUMEイベントpushのような
// 「自動で走る」経路だけがこのフラグを見る。
fn sync_paused_flag_path() -> std::path::PathBuf {
    crate::data_dir().join("sync_paused")
}

pub fn is_sync_paused() -> bool {
    sync_paused_flag_path().exists()
}

pub fn set_sync_paused(paused: bool) -> Result<(), String> {
    let path = sync_paused_flag_path();
    if paused {
        std::fs::write(&path, "").map_err(|e| e.to_string())?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    eprintln!("{} set_sync_paused: {}", TAG, paused);
    Ok(())
}

// クラウドの「世代番号」（GAS側でLockServiceにより排他的に払い出される、
// 「クラウドも含めて全データ削除」「データを圧縮」のたびに1つ進むカウンタ）に
// 関する処理。全体リセット系操作が伝播したかを判定する唯一の手段
// （以前併用していたタイムスタンプ比較のHARD_RESETマーカーは、2端末の時計が
// ズレている場合に誤判定する上、世代が一致していても「復元」等が書き込む
// 古いタイムスタンプのデータを誤って破棄する副作用があったため廃止した）。
fn local_generation_path() -> std::path::PathBuf {
    crate::data_dir().join("generation.txt")
}

// パスを引数に取る形にして、テストが実データのgeneration.txtに触れず
// 一時ファイルで検証できるようにする（merge_into_localの`path`引数と同じ考え方）。
fn read_generation_at(path: &std::path::Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn write_generation_at(path: &std::path::Path, gen: u64) {
    let _ = std::fs::write(path, gen.to_string());
}

fn save_local_generation(gen: u64) { write_generation_at(&local_generation_path(), gen) }

// クラウドの現在の世代番号を取得する。取得失敗時（オフライン等）はNoneを返し、
// 呼び出し側は世代ゲートを素通りさせて通常のunionマージ(merge_into_local)に任せる。
fn fetch_cloud_generation(base_url: &str, secret: &str) -> Option<u64> {
    let url = format!("{}?secret={}&action=get_generation", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    resp.text().ok()?.trim().parse().ok()
}

// クラウドの世代がローカルより新しければ、Driveの内容を通常マージせず丸ごと
// 採用する（この端末が「全削除/圧縮」を知らない間にもう一方の端末がそれを
// 実行した場合、ローカル未同期分を破棄してクラウド側を正とする）。世代が
// 同じ・取得失敗時は、通常のunionマージ(merge_into_local)に任せる。
fn merge_or_adopt_at(path: &std::path::Path, drive_content: &str, cloud_gen: Option<u64>, gen_path: &std::path::Path) -> bool {
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

fn merge_or_adopt(path: &std::path::Path, drive_content: &str, cloud_gen: Option<u64>) -> bool {
    merge_or_adopt_at(path, drive_content, cloud_gen, &local_generation_path())
}

// push直前にクラウドの世代が変わっていないか再確認する。変わっていれば、
// pull時に見た内容を基にした今回のマージ結果は既に古くなっている可能性があるため
// pushを見送る（false）。次の同期サイクルで新しい世代を検知し、やり直しがきく。
fn generation_unchanged_since(base_url: &str, secret: &str, observed_gen: Option<u64>) -> bool {
    match (observed_gen, fetch_cloud_generation(base_url, secret)) {
        (Some(old), Some(now)) if now > old => {
            eprintln!("{} generation_unchanged_since: cloud generation advanced ({} → {}) mid-sync — skip push this cycle", TAG, old, now);
            false
        }
        _ => true,
    }
}

pub fn pull_mobile_events_inner() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} pull_mobile_events #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "スキップ (設定なし)".into(),
    };

    let client = match crate::http_client() {
        Ok(c) => c,
        Err(e) => {
            let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("{} pull_mobile_events #{}: error ({}回連続) {}", TAG, n, ec, e);
            if ec >= 3 {
                eprintln!("{} pull_mobile_events #{}: WARN {}回連続失敗 — ネットワーク障害の可能性", TAG, n, ec);
            }
            return format!("クライアントエラー: {}", e);
        }
    };

    let url = format!("{}?secret={}", base_url.trim_end_matches('/'), secret);
    let resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("{} pull_mobile_events #{}: error ({}回連続) {}", TAG, n, ec, e);
            if ec >= 3 {
                eprintln!("{} pull_mobile_events #{}: WARN {}回連続失敗 — ネットワーク障害の可能性", TAG, n, ec);
            }
            return format!("取得失敗: {}", e);
        }
    };
    if !resp.status().is_success() {
        let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        let status = resp.status().as_u16();
        eprintln!("{} pull_mobile_events #{}: error ({}回連続) HTTP {}", TAG, n, ec, status);
        return format!("HTTP {}", status);
    }
    let content = match resp.text() {
        Ok(t) => t.trim().to_string(),
        Err(e) => return format!("レスポンス読み取り失敗: {}", e),
    };

    if content.is_empty() || content == "Unauthorized" {
        if content == "Unauthorized" { return "認証失敗（シークレットを確認）".into(); }
        return "モバイルイベントなし".into();
    }

    let kb = content.len() as f64 / 1024.0;
    let mut msgs = Vec::new();
    let mut new_events = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        match apply_mobile_event_line(line) {
            Ok(msg) => {
                if msg.starts_with("追加") { new_events += 1; }
                msgs.push(msg);
            }
            Err(e) => msgs.push(format!("エラー: {}", e)),
        }
    }

    if new_events > 0 {
        let events_path = crate::data_dir().join("sleep_events.txt");
        let _ = sort_events_file(&events_path);
        *SESSION_CACHE.lock().unwrap() = None;
    }

    CONSECUTIVE_ERRORS.store(0, Ordering::Relaxed);
    let ms = t0.elapsed().as_millis();

    if msgs.is_empty() {
        eprintln!("{} pull_mobile_events #{}: {:.1}KB received, 0 events processed  (+{}ms)", TAG, n, kb, ms);
        return "モバイルイベントなし".into();
    }

    eprintln!("{} pull_mobile_events #{}: {:.1}KB received, {} events processed  (+{}ms)", TAG, n, kb, new_events, ms);
    format!("{} 件処理: {}", msgs.len(), msgs.join(" / "))
}

// GAS側は検証NG時（HTML混入・縮小ガード等）でもHTTP 200で"error: ..."を返す
// ため、ステータスだけでなく本文も確認する必要がある。
pub fn backup_to_drive(content: &str) -> String {
    backup_to_drive_ex(content, false, None)
}

// 「データを圧縮」「クラウドも含めて全データ削除」のように意図的に大きく
// 縮小した内容をpushする経路専用。GAS側の縮小ガードをforce=1で回避する。
pub fn backup_to_drive_forced(content: &str) -> String {
    backup_to_drive_ex(content, true, None)
}

// pull直後に取得した内容のハッシュをexpected_hashとして渡すことで、pull〜push間に
// 別端末（や手動操作）が割り込んで書き込んだ場合、GAS側が拒否してくれる
// （楽観的並行性制御。詳細はworker/appsscript.gsの「G. 内容ハッシュ」参照）。
// 拒否された場合はbody.trim().starts_with("error:")のため呼び出し側の既存の
// エラーハンドリングにそのまま乗る（"Drive拒否: error: conflict: ..."のような形）。
pub fn backup_to_drive_checked(content: &str, expected_hash: &str) -> String {
    backup_to_drive_ex(content, false, Some(expected_hash))
}

fn backup_to_drive_ex(content: &str, force: bool, expected_hash: Option<&str>) -> String {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Driveスキップ(未設定)".into(),
    };

    let force_param = if force { "&force=1".to_string() } else { String::new() };
    let hash_param = expected_hash.map(|h| format!("&expected_hash={h}")).unwrap_or_default();
    let url = format!("{}?secret={}&action=backup{}{}", base_url.trim_end_matches('/'), secret, force_param, hash_param);
    let kb = content.len() as f64 / 1024.0;
    let resp = match crate::http_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} ERROR backup_to_drive: {}", TAG, e);
            return format!("Drive送信失敗: {}", e);
        }
    };

    let ms = t0.elapsed().as_millis();
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        eprintln!("{} ERROR backup_to_drive: HTTP {}  (+{}ms)", TAG, status, ms);
        return format!("Drive HTTP {}", status);
    }
    let body = resp.text().unwrap_or_default();
    if body.trim().starts_with("error:") {
        eprintln!("{} ERROR backup_to_drive: rejected by server: {}  (+{}ms)", TAG, body.trim(), ms);
        return format!("Drive拒否: {}", body.trim());
    }
    eprintln!("{} backup_to_drive: {:.1}KB sent  (+{}ms)", TAG, kb, ms);
    "Drive バックアップ完了".into()
}

// Drive上のバックアップファイル（sleep_events_backup.txt/sleep_manual_backup.txt）と
// eventsシートの行を全消去する。ローカルファイルの削除は呼び出し側が別途行う。
pub fn clear_cloud_data() -> Result<(), String> {
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    // confirm=yes はGAS側の誤爆防止ガード。省略すると"error: clear_all requires
    // confirm=yes"が返るだけで実際の削除は行われない。
    let url = format!("{}?secret={}&action=clear_all&confirm=yes", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client()?
        .post(&url)
        .header("Content-Length", "0")
        .body("")
        .send()
        .map_err(|e| format!("送信失敗: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let body = resp.text().unwrap_or_default();
    let body = body.trim();
    if body.starts_with("error:") {
        eprintln!("{} ERROR clear_cloud_data: rejected by server: {}", TAG, body);
        return Err(body.to_string());
    }
    // action=clear_all は新しい世代番号を返す（GAS側でLockServiceにより排他的に
    // 払い出される）。ここで確定させ、以後のpushをこの端末が「最新」として行える
    // ようにする。
    if let Ok(new_gen) = body.parse::<u64>() {
        save_local_generation(new_gen);
        eprintln!("{} clear_cloud_data: done (generation={})", TAG, new_gen);
    } else {
        eprintln!("{} clear_cloud_data: done", TAG);
    }
    Ok(())
}

// クラウド全削除。action=clear_allだけでは信頼性に難があり、特にsleep_manual.txt
// 側のバックアップが消えないケースを確認している。そのため削除後にローカル
// （events::clear_all_dataで空になっている）の内容を直接pushして確実に上書きする
// （push_authoritative_content_to_driveと同じ「マージせず直接反映」パターン）。
pub fn clear_cloud_data_and_push_reset() -> Result<(), String> {
    clear_cloud_data()?;
    let events_content = std::fs::read_to_string(crate::data_dir().join("sleep_events.txt")).unwrap_or_default();
    let manual_content = std::fs::read_to_string(crate::data_dir().join("sleep_manual.txt")).unwrap_or_default();
    // clear_all直後はほぼ空の内容をpushするため、GAS側の縮小ガードをforceで回避する。
    let events_msg = backup_to_drive_forced(&events_content);
    let manual_msg = backup_manual_to_drive_forced(&manual_content);
    eprintln!("{} clear_cloud_data_and_push_reset: events={} manual={}", TAG, events_msg, manual_msg);
    Ok(())
}

// events::compact_data() で作り直したsleep_events.txtを「新しい正の状態」として
// クラウドにも反映する。通常のsync（Drive→localマージ→push）だと、Driveに残っている
// 圧縮前の内容が通常のunionマージで復活し、圧縮結果が台無しになってしまう。
// そのためここでは先にclear_all（Driveのバックアップファイル・スプレッドシートの
// events行を全消去＋世代番号を新しく進める）してから、圧縮後の内容をマージなしで
// 直接pushする。これにより次回同期時は世代が一致し、この内容がそのまま正として扱われる。
pub fn push_authoritative_content_to_drive(events_content: &str) -> Result<(), String> {
    clear_cloud_data()?;
    // 圧縮・復元は既存の内容と大きさが大きく異なりうるため、GAS側の縮小ガードをforceで回避する。
    let events_msg = backup_to_drive_forced(events_content);
    let manual_content = std::fs::read_to_string(crate::data_dir().join("sleep_manual.txt")).unwrap_or_default();
    let manual_msg = backup_manual_to_drive_forced(&manual_content);
    eprintln!("{} push_authoritative_content_to_drive: events={} manual={}", TAG, events_msg, manual_msg);
    Ok(())
}

// Download raw sleep_events.txt content from Drive. Returns None on error / empty / unauthorized.
// GAS側（worker/appsscript.gs）と同じ検証をクライアント側でも独立に行う。
// GAS自身が"error: ..."を返してくれるケース（保存済み内容が壊れていると
// GAS自身が判断した場合）はそれで弾けるが、Googleの認証リダイレクト等
// GASのスクリプトロジックを経由せずに割り込まれるケース（実際に発生した、
// ログインページのHTMLがそのままsleep_events_backup.txtに混入した事故）は
// GAS側の検証をすり抜けるため、クライアント側でも中身を見て判断する必要がある。
fn looks_like_html_or_js(content: &str) -> bool {
    let head: String = content.chars().take(5000).collect::<String>().to_lowercase();
    ["<!doctype", "<html", "<head", "<script", "<meta", "(function()", "document.queryselector"]
        .iter()
        .any(|pat| head.contains(pat))
}

// sleep_events.txtの形式検証: 各行 "YYYY-MM-DD HH:MM:SS,TAG"。
// 非空行の90%以上が形式に一致しなければ不正とみなす（GAS側looksLikeEventsContent_と同じ基準）。
fn looks_like_events_content(content: &str) -> bool {
    if looks_like_html_or_js(content) { return false; }
    let lines: Vec<&str> = content.lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() { return false; }
    let valid = lines.iter().filter(|l| is_event_line(l)).count();
    (valid as f64 / lines.len() as f64) >= 0.9
}

fn is_event_line(line: &str) -> bool {
    if line.len() < 21 { return false; }
    if !is_timestamp_like(&line[..19]) { return false; }
    if line.as_bytes()[19] != b',' { return false; }
    let rest = &line[20..];
    !rest.is_empty() && !rest.contains('<') && !rest.contains('>')
}

fn is_timestamp_like(ts: &str) -> bool {
    let b = ts.as_bytes();
    if b.len() != 19 { return false; }
    let d = |i: usize| b[i].is_ascii_digit();
    d(0) && d(1) && d(2) && d(3) && b[4] == b'-'
        && d(5) && d(6) && b[7] == b'-'
        && d(8) && d(9) && b[10] == b' '
        && d(11) && d(12) && b[13] == b':'
        && d(14) && d(15) && b[16] == b':'
        && d(17) && d(18)
}

fn fetch_drive_events(base_url: &str, secret: &str) -> Option<String> {
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    let text = resp.text().ok()?;
    let t = text.trim();
    if t.is_empty() || t == "Unauthorized" || t.starts_with("not found") { return None; }
    if t.starts_with("error:") {
        eprintln!("{} fetch_drive_events: ERROR server-side backup rejected: {}", TAG, t);
        return None;
    }
    if !looks_like_events_content(t) {
        eprintln!("{} fetch_drive_events: ERROR content failed client-side validation (HTML/JS混入またはイベント形式不一致) — discarding", TAG);
        return None;
    }
    Some(text)
}

// Download sleep_manual.txt content from Drive.
// sleep_manual.txtは自由形式のため、HTML/JS混入チェックのみ行う（GAS側と同じ基準）。
fn fetch_drive_manual(base_url: &str, secret: &str) -> Option<String> {
    let url = format!("{}?secret={}&action=restore_manual", base_url.trim_end_matches('/'), secret);
    let resp = crate::http_client().ok()?.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }
    let text = resp.text().ok()?;
    let t = text.trim();
    if t.is_empty() || t == "Unauthorized" || t.starts_with("not found") { return None; }
    if t.starts_with("error:") {
        eprintln!("{} fetch_drive_manual: ERROR server-side backup rejected: {}", TAG, t);
        return None;
    }
    if looks_like_html_or_js(t) {
        eprintln!("{} fetch_drive_manual: ERROR content looks like HTML/JS — discarding", TAG);
        return None;
    }
    Some(text)
}

fn backup_manual_to_drive(content: &str) -> String {
    backup_manual_to_drive_ex(content, false, None)
}

// backup_to_drive_forced と同様、意図的な縮小pushでGAS側の縮小ガードを回避する。
fn backup_manual_to_drive_forced(content: &str) -> String {
    backup_manual_to_drive_ex(content, true, None)
}

// backup_to_drive_checked と同様の楽観的並行性制御（詳細はそちらのコメント参照）。
fn backup_manual_to_drive_checked(content: &str, expected_hash: &str) -> String {
    backup_manual_to_drive_ex(content, false, Some(expected_hash))
}

fn backup_manual_to_drive_ex(content: &str, force: bool, expected_hash: Option<&str>) -> String {
    let t0 = std::time::Instant::now();
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return "Manual Driveスキップ(未設定)".into(),
    };
    let force_param = if force { "&force=1".to_string() } else { String::new() };
    let hash_param = expected_hash.map(|h| format!("&expected_hash={h}")).unwrap_or_default();
    let url = format!("{}?secret={}&action=backup_manual{}{}", base_url.trim_end_matches('/'), secret, force_param, hash_param);
    let kb = content.len() as f64 / 1024.0;
    let resp = match crate::http_client()
        .and_then(|c| c.post(&url).header("Content-Type", "text/plain").body(content.to_string()).send().map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => { eprintln!("{} ERROR backup_manual_to_drive: {}", TAG, e); return format!("Manual Drive送信失敗: {}", e); }
    };
    let ms = t0.elapsed().as_millis();
    if !resp.status().is_success() {
        return format!("Manual Drive HTTP {}", resp.status().as_u16());
    }
    let body = resp.text().unwrap_or_default();
    if body.trim().starts_with("error:") {
        eprintln!("{} ERROR backup_manual_to_drive: rejected by server: {}  (+{}ms)", TAG, body.trim(), ms);
        return format!("Manual Drive拒否: {}", body.trim());
    }
    eprintln!("{} backup_manual_to_drive: {:.1}KB sent  (+{}ms)", TAG, kb, ms);
    "Manual Drive バックアップ完了".into()
}

// Merge drive_content lines into the local file (sort by timestamp, dedup).
// Returns true if the local file was updated (new lines added from Drive).
fn merge_into_local(path: &std::path::Path, drive_content: &str) -> bool {
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

pub fn sync_gist() -> Result<String, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} sync_gist #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let events_path = crate::data_dir().join("sleep_events.txt");
    let manual_path = crate::data_dir().join("sleep_manual.txt");

    let cfg = load_config_inner();
    let url_secret = if let (Some(u), Some(s)) = (cfg.mobile_url, cfg.mobile_secret) {
        if !u.is_empty() && !s.is_empty() { Some((u, s)) } else { None }
    } else { None };

    // 0. Drive → local merge (sleep_events.txt and sleep_manual.txt)
    let cloud_gen = url_secret.as_ref().and_then(|(u, s)| fetch_cloud_generation(u, s));
    if let Some((ref u, ref s)) = url_secret {
        if let Some(drive_content) = fetch_drive_events(u, s) {
            let kb = drive_content.len() as f64 / 1024.0;
            eprintln!("{} sync_gist #{}: {:.1}KB events from Drive", TAG, n, kb);
            if merge_or_adopt(&events_path, &drive_content, cloud_gen) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
        }
        if let Some(drive_manual) = fetch_drive_manual(u, s) {
            let kb = drive_manual.len() as f64 / 1024.0;
            eprintln!("{} sync_gist #{}: {:.1}KB manual from Drive", TAG, n, kb);
            if merge_or_adopt(&manual_path, &drive_manual, cloud_gen) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
            let _ = sort_manual_file(&manual_path);
        }
    }

    // 1. Pull mobile events from Google Sheets
    let pull_msg = pull_mobile_events_inner();

    // 2. Sort+dedup sleep_events.txt
    if events_path.exists() {
        let _ = sort_events_file(&events_path);
    }

    // 3. push直前に世代が変わっていないか再確認する。pull時点より進んでいたら、
    // このマージ結果は既に古くなっている可能性があるためpushを見送る。
    let should_push = url_secret.as_ref()
        .map(|(u, s)| generation_unchanged_since(u, s, cloud_gen))
        .unwrap_or(true);
    let drive_msg = if should_push {
        // Read updated sleep_events.txt and upload both files to Drive (local → Drive)
        let content = if events_path.exists() {
            std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?
        } else {
            String::new()
        };
        let drive_msg = backup_to_drive(&content);
        if let Ok(manual_content) = std::fs::read_to_string(&manual_path) {
            let _ = backup_manual_to_drive(&manual_content);
        }
        drive_msg
    } else {
        "スキップ (クラウドが更新済み)".into()
    };

    let ms = t0.elapsed().as_millis();
    eprintln!("{} sync_gist #{}: done  (+{}ms)", TAG, n, ms);
    Ok(format!("同期完了 — モバイル: {} / {}", pull_msg, drive_msg))
}

pub fn ensure_events_from_drive() {
    let path = crate::data_dir().join("sleep_events.txt");
    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => {
            if !path.exists() {
                eprintln!("{} ensure_events_from_drive: cloud not configured and no local file", TAG);
            }
            return;
        }
    };

    eprintln!("{} ensure_events_from_drive: fetching from Drive", TAG);
    let t0 = std::time::Instant::now();
    match fetch_drive_events(&base_url, &secret) {
        Some(drive_content) => {
            let kb = drive_content.len() as f64 / 1024.0;
            let cloud_gen = fetch_cloud_generation(&base_url, &secret);
            merge_or_adopt(&path, &drive_content, cloud_gen);
            eprintln!("{} ensure_events_from_drive: {:.1}KB  (+{}ms)", TAG, kb, t0.elapsed().as_millis());
        }
        None => {
            eprintln!("{} ensure_events_from_drive: Drive unavailable  (+{}ms)", TAG, t0.elapsed().as_millis());
        }
    }
}

// Back up sleep_events.txt to Drive after writes.
// Drive→ローカルマージ → push することで、他デバイスがDriveに書き込んだイベントの上書き消失を防ぐ。
// 呼び出し側で std::thread::spawn すること。
pub fn auto_backup_after_event(events_path: &std::path::Path) {
    // 1. Driveからマージ（他デバイスのイベントを取り込む）
    let cfg = load_config_inner();
    let url_secret = if let (Some(u), Some(s)) = (cfg.mobile_url.clone(), cfg.mobile_secret.clone()) {
        if !u.is_empty() && !s.is_empty() { Some((u, s)) } else { None }
    } else { None };
    let cloud_gen = url_secret.as_ref().and_then(|(u, s)| fetch_cloud_generation(u, s));
    // pull時点の内容のハッシュを覚えておき、push時にexpected_hashとして送る
    // （pull〜push間に別端末が割り込んで書き込んだ場合、GAS側で拒否させるため。
    // fetch自体に失敗した場合はNoneのままとなり、従来通りチェック無しでpushする）。
    let mut pulled_hash: Option<String> = None;
    if let Some((ref u, ref s)) = url_secret {
        if let Some(drive_content) = fetch_drive_events(u, s) {
            pulled_hash = Some(sha256_hex(&drive_content));
            if merge_or_adopt(events_path, &drive_content, cloud_gen) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
        }
    }
    // 2. push直前に世代が変わっていないか再確認してからDriveにpush
    let should_push = url_secret.as_ref()
        .map(|(u, s)| generation_unchanged_since(u, s, cloud_gen))
        .unwrap_or(true);
    if should_push {
        if let Ok(content) = std::fs::read_to_string(events_path) {
            let msg = match &pulled_hash {
                Some(h) => backup_to_drive_checked(&content, h),
                None => backup_to_drive(&content),
            };
            eprintln!("{} auto_backup: {}", TAG, msg);
        }
    } else {
        eprintln!("{} auto_backup: skip upload (cloud generation advanced mid-sync)", TAG);
    }
}

// Back up sleep_manual.txt to Drive after writes. 呼び出し側で std::thread::spawn すること。
pub fn auto_backup_manual(manual_path: &std::path::Path) {
    if let Ok(content) = std::fs::read_to_string(manual_path) {
        let msg = backup_manual_to_drive(&content);
        eprintln!("{} auto_backup_manual: {}", TAG, msg);
    }
}

// Core sync logic shared by Android startup, focus events, and the "今すぐ同期" button.
// Merge Drive → local → pull Sheet → sort → upload → parse sessions.
// Returns cached sessions immediately if another sync is already in progress.
pub fn sync_mobile_inner() -> Vec<super::events::Session> {
    static N: AtomicU64 = AtomicU64::new(0);

    if SYNC_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        eprintln!("{} sync_mobile_inner: already running — returning cache", TAG);
        return SESSION_CACHE.lock().unwrap()
            .as_ref().map(|c| c.sessions.clone()).unwrap_or_default();
    }

    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} sync_mobile_inner #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let events_path = crate::data_dir().join("sleep_events.txt");
    let manual_path = crate::data_dir().join("sleep_manual.txt");

    // 1. Fetch settings (update THRESHOLD_SECS from Drive)
    let _ = super::config::fetch_settings_from_cloud();

    // 2. Drive → local merge (sleep_events.txt and sleep_manual.txt)
    let cfg = load_config_inner();
    let url_secret = if let (Some(u), Some(s)) = (cfg.mobile_url, cfg.mobile_secret) {
        if !u.is_empty() && !s.is_empty() { Some((u, s)) } else { None }
    } else { None };
    let cloud_gen = url_secret.as_ref().and_then(|(u, s)| fetch_cloud_generation(u, s));
    // pull時点の内容のハッシュを覚えておき、push時にexpected_hashとして送る
    // （pull〜push間に別端末が割り込んで書き込んだ場合、GAS側で拒否させるため。
    // fetch自体に失敗した場合はNoneのままとなり、従来通りチェック無しでpushする）。
    let mut events_pulled_hash: Option<String> = None;
    let mut manual_pulled_hash: Option<String> = None;
    if let Some((ref u, ref s)) = url_secret {
        if let Some(drive_content) = fetch_drive_events(u, s) {
            let kb = drive_content.len() as f64 / 1024.0;
            eprintln!("{} sync_mobile_inner #{}: {:.1}KB events from Drive", TAG, n, kb);
            events_pulled_hash = Some(sha256_hex(&drive_content));
            if merge_or_adopt(&events_path, &drive_content, cloud_gen) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
        }
        if let Some(drive_manual) = fetch_drive_manual(u, s) {
            let kb = drive_manual.len() as f64 / 1024.0;
            eprintln!("{} sync_mobile_inner #{}: {:.1}KB manual from Drive", TAG, n, kb);
            manual_pulled_hash = Some(sha256_hex(&drive_manual));
            if merge_or_adopt(&manual_path, &drive_manual, cloud_gen) {
                *SESSION_CACHE.lock().unwrap() = None;
            }
            let _ = sort_manual_file(&manual_path);
        }
    }

    // 3. Pull mobile events from Sheet (LEAVE_HOME / ARRIVE_HOME / APP_USAGE_START / APP_USAGE_END)
    pull_mobile_events_inner();

    // 4. Sort + dedup
    if events_path.exists() {
        let _ = sort_events_file(&events_path);
    }

    // 5. push直前に世代が変わっていないか再確認する。pull時点より進んでいたら、
    // このマージ結果は既に古くなっている可能性があるためpushを見送る。
    let should_push = url_secret.as_ref()
        .map(|(u, s)| generation_unchanged_since(u, s, cloud_gen))
        .unwrap_or(true);
    if should_push {
        if let Ok(content) = std::fs::read_to_string(&events_path) {
            let drive_msg = match &events_pulled_hash {
                Some(h) => backup_to_drive_checked(&content, h),
                None => backup_to_drive(&content),
            };
            eprintln!("{} sync_mobile_inner #{}: upload events: {}", TAG, n, drive_msg);
        }
        if let Ok(manual_content) = std::fs::read_to_string(&manual_path) {
            let manual_msg = match &manual_pulled_hash {
                Some(h) => backup_manual_to_drive_checked(&manual_content, h),
                None => backup_manual_to_drive(&manual_content),
            };
            eprintln!("{} sync_mobile_inner #{}: upload manual: {}", TAG, n, manual_msg);
        }
    } else {
        eprintln!("{} sync_mobile_inner #{}: skip upload (cloud generation advanced mid-sync)", TAG, n);
    }

    // 6. Parse sessions (rebuild cache)
    let sessions = parse_sessions_rust().unwrap_or_default();
    let mtime_events = events_path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    let mtime_manual = manual_path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    let mtime = mtime_events.max(mtime_manual);
    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions: sessions.clone(), mtime });

    let ms = t0.elapsed().as_millis();
    eprintln!("{} sync_mobile_inner #{}: {} sessions  (+{}ms)", TAG, n, sessions.len(), ms);

    SYNC_IN_PROGRESS.store(false, Ordering::SeqCst);
    sessions
}

pub fn fetch_from_cloud() -> Result<Vec<super::events::Session>, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("{} fetch_from_cloud #{}: started", TAG, n);
    let t0 = std::time::Instant::now();

    let cfg = load_config_inner();
    let (base_url, secret) = match (cfg.mobile_url, cfg.mobile_secret) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() => (u, s),
        _ => return Err("クラウド接続が未設定です".into()),
    };
    let url = format!("{}?secret={}&action=restore", base_url.trim_end_matches('/'), secret);
    let client = crate::http_client()?;
    let resp = client.get(&url).send().map_err(|e| {
        let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!("{} fetch_from_cloud #{}: error ({}回連続) {}", TAG, n, ec, e);
        if ec >= 3 {
            eprintln!("{} fetch_from_cloud #{}: WARN {}回連続失敗 — ネットワーク障害の可能性", TAG, n, ec);
        }
        format!("取得失敗: {}", e)
    })?;
    if !resp.status().is_success() {
        let ec = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        let status = resp.status().as_u16();
        eprintln!("{} fetch_from_cloud #{}: error ({}回連続) HTTP {}", TAG, n, ec, status);
        return Err(format!("HTTP {}", status));
    }
    let content = resp.text().map_err(|e| format!("レスポンス読み取り失敗: {}", e))?;
    if content.trim().is_empty() || content.trim() == "Unauthorized" {
        if content.trim() == "Unauthorized" {
            return Err("認証失敗（シークレットを確認）".into());
        }
        return Ok(vec![]);
    }
    // Driveから取得した内容をローカルにマージ（上書きではなく統合）
    let kb = content.len() as f64 / 1024.0;
    let path = crate::data_dir().join("sleep_events.txt");
    let cloud_gen = fetch_cloud_generation(&base_url, &secret);
    merge_or_adopt(&path, &content, cloud_gen);
    // Invalidate and rebuild cache
    *SESSION_CACHE.lock().unwrap() = None;
    let sessions = parse_sessions_rust()?;
    let mtime = path.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    *SESSION_CACHE.lock().unwrap() = Some(SessionCache { sessions: sessions.clone(), mtime });

    CONSECUTIVE_ERRORS.store(0, Ordering::Relaxed);
    let ms = t0.elapsed().as_millis();
    eprintln!("{} fetch_from_cloud #{}: {:.1}KB received  (+{}ms)", TAG, n, kb, ms);
    Ok(sessions)
}

pub fn test_mobile_connection(mobile_url: String, mobile_secret: String) -> Result<String, String> {
    if mobile_url.is_empty() || mobile_secret.is_empty() {
        return Err("URL とシークレットを入力してください".to_string());
    }
    let url = format!("{}?secret={}&action=health", mobile_url.trim_end_matches('/'), mobile_secret);
    let resp = crate::http_client()?
        .get(&url)
        .send()
        .map_err(|e| format!("ネットワークエラー: {}", e))?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status.is_success() && body.trim() == "ok" {
        Ok("接続成功".to_string())
    } else if body.trim() == "Unauthorized" {
        Err("認証失敗（シークレットを確認）".to_string())
    } else {
        Err(format!("HTTP {} — レスポンス: {}", status.as_u16(), body.trim()))
    }
}

#[cfg(test)]
mod cloud_tests;
