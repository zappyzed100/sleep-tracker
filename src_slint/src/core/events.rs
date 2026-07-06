//! events.rs — sleep_events.txt のパース・セッション管理
//!
//! 役割 : sleep_events.txt を読み込んで Session に変換するステートマシン実装。
//!        セッションの追加・削除・エクスポートなど全データ操作を担当する。
//!        SESSION_CACHE でパース結果をキャッシュしパフォーマンスを確保する。
//!        Tauri版 src-tauri/src/events.rs の移植（#[tauri::command] を除去しただけ）。
//!
//! 依存 : crate::data_dir, crate::THRESHOLD_SECS, crate::cloud, chrono
//! 公開 : `Session`, `SessionCache`, `SESSION_CACHE`, `parse_sessions_rust`,
//!        `sort_events_file`, `get_sessions`, `add_session`, `delete_session`,
//!        `get_events_content`, `restore_events`, `clear_all_data`, `compact_data`,
//!        `current_sleep_start`, `maybe_auto_backup`, `clear_backups`, `list_backups`,
//!        `export_csv`, `write_csv_file`, `import_csv`,
//!        `is_out_from_content`, `apply_mobile_event_line`,
//!        `excluded_dates_from_content`, `get_excluded_dates`, `set_day_excluded`,
//!        `HARD_RESET_TAG`

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

const TAG: &str = "[events]";

// 「全データ削除」「データを圧縮」のような"上書き"系操作が実行されたことを示す
// マーカー行（"{timestamp},HARD_RESET"）。PC/Androidは互いのローカルファイルを
// 直接は見えず、Drive経由の通常同期はunion（和集合）マージのため、一方が削除・圧縮
// してDriveを更新しても、もう一方が古いローカルを持ったまま同期すると削除・圧縮前の
// データが復活してDriveに書き戻ってしまう問題があった。
// このマーカーをローカル書き換え時に必ず残し、Driveへも通常のバックアップ経路で
// push することで、もう一方の端末は次回の同期（自動同期は数分以内、手動なら
// その時）でこのタグを検知し、マーカーの時刻以前のローカル専用行を破棄した上で
// マージするようになる（cloud::merge_into_local参照）。何度読んでも同じ結果になる
// （冪等）ため「このマーカーは処理済みか」の状態管理は不要。
pub const HARD_RESET_TAG: &str = "HARD_RESET";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub start: String,
    pub end: String,
    #[serde(rename = "duration")]
    pub duration_hours: f64,
    #[serde(rename = "type")]
    pub session_type: String,
    // その日が計測対象外（平均睡眠・予測計算から除外）としてマークされているか。
    // 睡眠時間そのものは記録として残す（グラフのバーは表示する）が、
    // 統計計算だけから除外する。#[serde(default)]で既存データとの互換性を保つ。
    #[serde(default)]
    pub excluded: bool,
}

pub struct SessionCache {
    pub sessions: Vec<Session>,
    pub mtime: std::time::SystemTime,
}

pub static SESSION_CACHE: std::sync::Mutex<Option<SessionCache>> = std::sync::Mutex::new(None);

// sleep_events.txt への読み書きを保護するミューテックス。
// sort_events_file（全行読み込み→ソート→全行書き込み）と append_event（1行追加）の
// 競合によるデータ消失を防ぐ。
pub static EVENTS_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// Returns true if the last OUT_START in the file has no matching OUT_END / IN_HOUSE.
pub fn is_out_from_content(content: &str) -> bool {
    let mut out = false;
    for line in content.lines() {
        if let Some(c) = line.trim().find(',') {
            match &line.trim()[c + 1..] {
                "OUT_START" => out = true,
                "OUT_END" | "IN_HOUSE" | "DEVICE_ON" | "APP_USAGE_START" => out = false,
                _ => {}
            }
        }
    }
    out
}

// 「YYYY-MM-DD」を計測対象外（平均睡眠・予測計算から除外）としてマークされている
// 日付の集合として返す。マーカーは通常のイベント行と同じ書式
// "タイムスタンプ,DAY_EXCLUDED:YYYY-MM-DD" / "...,DAY_INCLUDED:YYYY-MM-DD" で
// ファイルに追記され、同じ日付に対する最後のマーカーが有効になる
// （タイムスタンプは実際に操作した時刻を使うため、ファイルソート後も
// 時系列どおりの「最後の操作が勝つ」が保たれる）。
pub fn excluded_dates_from_content(content: &str) -> std::collections::HashSet<String> {
    let mut state: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}');
        let Some(c) = line.find(',') else { continue };
        let rest = &line[c + 1..];
        if let Some(date) = rest.strip_prefix("DAY_EXCLUDED:") {
            state.insert(date.to_string(), true);
        } else if let Some(date) = rest.strip_prefix("DAY_INCLUDED:") {
            state.insert(date.to_string(), false);
        }
    }
    state.into_iter().filter(|(_, excluded)| *excluded).map(|(d, _)| d).collect()
}

// sleep_events.txtを直接読んで計測対象外の日付集合を返す。セッションが1件も
// 無い日（記録0h）でもチャート側で対象外表示ができるよう、Session一覧経由
// ではなくファイルから直接判定する（Session.excludedはセッションが存在する
// 日にしか付与されないため、0h の日はこちらでしか拾えない）。
pub fn get_excluded_dates() -> std::collections::HashSet<String> {
    let path = crate::data_dir().join("sleep_events.txt");
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    excluded_dates_from_content(&raw)
}

// 指定した日付(YYYY-MM-DD)を計測対象外/対象に切り替える。
pub fn set_day_excluded(date: &str, excluded: bool) -> Result<(), String> {
    let events_path = crate::data_dir().join("sleep_events.txt");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let marker = if excluded { "DAY_EXCLUDED" } else { "DAY_INCLUDED" };
    let line = format!("{},{}:{}\n", now, marker, date);
    let mut f = OpenOptions::new().create(true).append(true).open(&events_path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    drop(f);
    sort_events_file(&events_path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    let ep = events_path.clone();
    std::thread::spawn(move || { super::cloud::auto_backup_after_event(&ep); });
    Ok(())
}

pub fn sort_manual_file(path: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = content.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    lines.sort();
    lines.dedup();
    lines.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

pub fn sort_events_file(path: &std::path::Path) -> Result<(), String> {
    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    let t0 = std::time::Instant::now();
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = content.lines()
        .map(|l| l.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    // Sort by full content first so dedup removes ALL duplicates (including
    // same-timestamp pairs that would otherwise interleave and survive dedup).
    lines.sort();
    lines.dedup();
    // Re-sort by timestamp for chronological order.
    lines.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    let ms = t0.elapsed().as_millis();
    if ms > 100 {
        eprintln!("{} sort: SLOW +{}ms", TAG, ms);
    }
    Ok(())
}

// Parse and apply one "TAG,TIMESTAMP" line from mobile_event.txt.
pub fn apply_mobile_event_line(line: &str) -> Result<String, String> {
    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    let mut parts = line.splitn(2, ',');
    let tag      = parts.next().ok_or("フォーマット不正")?.trim();
    let time_raw = parts.next().ok_or("フォーマット不正")?.trim();

    let event_type = match tag {
        "LEAVE_HOME" | "LEAVE"                       => "OUT_START",
        "ARRIVE_HOME" | "ARRIVE"                     => "OUT_END",
        "DEVICE_ON"                                   => "DEVICE_ON",
        "APP_USAGE_START"                             => "APP_USAGE_START",
        "APP_USAGE_END"                               => "APP_USAGE_END",
        other                                         => return Err(format!("不明タグ: {}", other)),
    };

    let ts = if let Ok(ms) = time_raw.parse::<i64>() {
        use chrono::{Local, TimeZone};
        Local.timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| time_raw.to_string())
    } else {
        time_raw.to_string()
    };

    let new_line    = format!("{},{}", ts, event_type);
    let events_path = crate::data_dir().join("sleep_events.txt");

    let existing = if events_path.exists() {
        std::fs::read_to_string(&events_path).unwrap_or_default()
    } else {
        String::new()
    };

    if existing.lines().any(|l| l.trim() == new_line.as_str()) {
        return Ok(format!("重複スキップ: {}", new_line));
    }

    // Tablet activity while marked as out → insert IN_HOUSE to cancel the out-state.
    // is_out_from_content returns false once IN_HOUSE is in the file, so only one is inserted.
    let need_in_house = matches!(event_type, "DEVICE_ON" | "APP_USAGE_START") && is_out_from_content(&existing);

    // IN_HOUSE と DEVICE_ON を1回のファイルオープンで書き込む（競合防止）
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
        if need_in_house {
            let _ = writeln!(f, "{},IN_HOUSE", ts);
        }
        let _ = writeln!(f, "{}", new_line);
    } else {
        return Err("書き込み失敗".into());
    }

    if event_type == "DEVICE_ON" {
        let _ = std::fs::write(crate::data_dir().join("device_heartbeat.txt"), format!("{}\n", ts));
    }

    Ok(format!("追加: {}", new_line))
}

// APP_USAGE区間（タブレットのUsageStatsManager由来）のフィルタ用しきい値。
// scratchpad/sync_design_test で18シナリオ検証した設計をそのまま採用している。
const MIN_APP_USAGE_SECS: i64 = 60;           // これ未満の単発利用は「一瞬触れただけ」として無視
const APP_USAGE_MERGE_GAP_SECS: i64 = 120;    // この間隔以内の利用は1回の利用として統合してから判定する

// 近接するAPP_USAGE区間を統合し（画面ロック→即再開のような細切れ検知対策）、
// 統合後もなお短すぎる区間（通知を一瞬見ただけ等）は除外する。
fn coalesce_and_filter_app_usage(mut pairs: Vec<(i64, String, i64, String)>) -> Vec<(i64, String, i64, String)> {
    pairs.sort_by_key(|(s, _, _, _)| *s);
    let mut merged: Vec<(i64, String, i64, String)> = Vec::new();
    for (s, s_ts, e, e_ts) in pairs {
        if let Some(last) = merged.last_mut() {
            if s - last.2 <= APP_USAGE_MERGE_GAP_SECS {
                if e > last.2 {
                    last.2 = e;
                    last.3 = e_ts;
                }
                continue;
            }
        }
        merged.push((s, s_ts, e, e_ts));
    }
    merged.into_iter().filter(|(s, _, e, _)| e - s >= MIN_APP_USAGE_SECS).collect()
}

fn ts_to_epoch(s: &str) -> Option<i64> {
    use chrono::NaiveDateTime;
    let ndt = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
    Some(ndt.and_utc().timestamp())
}

fn epoch_to_ts(ep: i64) -> String {
    use chrono::DateTime;
    let dt = DateTime::from_timestamp(ep, 0).map(|d| d.naive_utc());
    dt.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

pub fn parse_sessions_rust() -> Result<Vec<Session>, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let events_path    = crate::data_dir().join("sleep_events.txt");
    let heartbeat_path = crate::data_dir().join("sleep_heartbeat.txt");
    let manual_path    = crate::data_dir().join("sleep_manual.txt");
    let min_sleep_secs = crate::THRESHOLD_SECS.load(Ordering::Relaxed) as i64;

    if !events_path.exists() {
        eprintln!("{} parse_sessions #{}: cache MISS — no file", TAG, n);
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?;
    let kb = raw.len() as f64 / 1024.0;
    let heartbeat_raw = std::fs::read_to_string(&heartbeat_path).ok();
    let manual_raw = std::fs::read_to_string(&manual_path).ok();

    let t0 = std::time::Instant::now();
    let (sessions, event_count) = parse_sessions_from_str(
        &raw,
        manual_raw.as_deref(),
        heartbeat_raw.as_deref(),
        min_sleep_secs,
    );
    let ms = t0.elapsed().as_millis();
    eprintln!("{} parse_sessions #{}: cache MISS — {} events → {} sessions ({:.1}KB)  (+{}ms)",
        TAG, n, event_count, sessions.len(), kb, ms);

    Ok(sessions)
}

// sleep_events.txt(+ sleep_manual.txt)の生テキストからセッション一覧を組み立てる、
// ファイルI/O・グローバル状態に依存しない純粋関数。parse_sessions_rust()から
// 実ファイルを読んで呼ばれるほか、テストからも直接呼べる。
// 戻り値は (セッション一覧, 生イベント行数)。
fn parse_sessions_from_str(
    raw: &str,
    manual_raw: Option<&str>,
    heartbeat_raw: Option<&str>,
    min_sleep_secs: i64,
) -> (Vec<Session>, usize) {
    // Heartbeat (for POWER_LOSS start-time correction)
    let mut hb_epoch: i64 = 0;
    let mut hb_idle_ms: i64 = 0;
    if let Some(txt) = heartbeat_raw {
        if let Some(line) = txt.lines().next() {
            let line = line.trim_end_matches('\r');
            if let Some(c) = line.find(',') {
                if let Some(ep) = ts_to_epoch(&line[..c]) {
                    hb_epoch = ep;
                    hb_idle_ms = line[c+1..].trim().parse().unwrap_or(0);
                }
            }
        }
    }

    // Collect soft-deleted session start timestamps before running the state machine.
    let mut deleted_starts: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}');
        if let Some(c) = line.find(',') {
            if &line[c+1..] == "SESSION_DELETED" {
                deleted_starts.insert(line[..c].to_string());
            }
        }
    }

    let excluded_dates = excluded_dates_from_content(raw);

    struct Ev { epoch: i64, ts: String, ty: String }
    let mut evs: Vec<Ev> = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}');
        if line.is_empty() { continue; }
        if let Some(c) = line.find(',') {
            if let Some(ep) = ts_to_epoch(&line[..c]) {
                evs.push(Ev { epoch: ep, ts: line[..c].to_string(), ty: line[c+1..].to_string() });
            }
        }
    }
    evs.sort_by_key(|e| e.epoch);
    let event_count = evs.len();

    // ── State machine: two-pass closed-pair algorithm ────────────────────────────
    //
    // 閉じていないペアは一切無視する（IN_HOUSE も使わない）。
    //   IDLE pair       : IDLE_START       → 次の IDLE_RESUME
    //   OUT pair        : OUT_START        → 次の OUT_END（iPhoneのGPS由来、信頼できる）
    //   APP_USAGE pair  : APP_USAGE_START  → 次の APP_USAGE_END
    //                     （タブレットのUsageStatsManager由来のアプリ利用区間）
    //
    // DEVICE_ONはもう睡眠セッションの分割には使わない。「タブレットに一瞬触れた」
    // だけでは起きていた証拠にならないため（詳細はplatform/android_bg.rsのコメント参照）。
    // 代わりにAPP_USAGE区間（実際にアプリを使っていた区間）をOUT区間と全く同じ
    // 「除外ギャップ」として扱う。DEVICE_ONは在宅/外出の解除（IN_HOUSE挿入）専用。
    //
    // Pass 1: 閉じたペアを収集する。
    // Pass 2: 各 IDLE ペア内から OUT 期間・APP_USAGE 期間で区切った細切れを作り、
    //         規定時間以上のものを睡眠セッションとして記録する。

    // Pass 1 ─────────────────────────────────────────────────────────────────────
    // (start_ep, start_ts, end_ep, end_ts)
    let mut idle_pairs:      Vec<(i64, String, i64, String)> = Vec::new();
    let mut out_pairs:       Vec<(i64, String, i64, String)> = Vec::new();
    let mut app_usage_pairs: Vec<(i64, String, i64, String)> = Vec::new();
    // POWER session tracking: (start_ep, start_ts, end_ep, end_ts, type)
    let mut power_sessions: Vec<(i64, String, i64, String, String)> = Vec::new();
    {
        let mut idle_pend:      Option<(i64, String)> = None;
        let mut out_pend:       Option<(i64, String)> = None;
        let mut app_usage_pend: Option<(i64, String)> = None;
        let mut power_pend:     Option<(i64, String)> = None;
        let mut prev_ep: i64 = 0;

        for ev in &evs {
            let (ep, ts, ty) = (ev.epoch, ev.ts.as_str(), ev.ty.as_str());
            match ty {
                "IDLE_START"  => { idle_pend  = Some((ep, ts.to_string())); }
                "IDLE_RESUME" => {
                    if let Some((sep, sts)) = idle_pend.take() {
                        idle_pairs.push((sep, sts, ep, ts.to_string()));
                    }
                }
                "OUT_START"   => { out_pend   = Some((ep, ts.to_string())); }
                "OUT_END"     => {
                    if let Some((oep, ots)) = out_pend.take() {
                        out_pairs.push((oep, ots, ep, ts.to_string()));
                    }
                }
                "APP_USAGE_START" => { app_usage_pend = Some((ep, ts.to_string())); }
                "APP_USAGE_END"   => {
                    if let Some((aep, ats)) = app_usage_pend.take() {
                        app_usage_pairs.push((aep, ats, ep, ts.to_string()));
                    }
                }
                "SUSPEND" | "SHUTDOWN" => {
                    if power_pend.is_none() {
                        power_pend = Some((ep, ts.to_string()));
                    }
                }
                "RESUME" | "STARTUP" => {
                    if let Some((pep, pts)) = power_pend.take() {
                        let dur = ep - pep;
                        if dur >= min_sleep_secs {
                            power_sessions.push((pep, pts, ep, ts.to_string(), "POWER".to_string()));
                        }
                    } else if prev_ep > 0 && ep - prev_ep > 4 * 3600 {
                        // 大きな空白 → POWER_LOSS
                        let (start_ep, start_ts_s) = if hb_epoch > 0 && hb_epoch > prev_ep && hb_epoch < ep {
                            let adj = hb_epoch - hb_idle_ms / 1000;
                            if adj > prev_ep { (adj, epoch_to_ts(adj)) } else { (prev_ep, epoch_to_ts(prev_ep)) }
                        } else {
                            (prev_ep, epoch_to_ts(prev_ep))
                        };
                        let dur = ep - start_ep;
                        if dur >= min_sleep_secs {
                            power_sessions.push((start_ep, start_ts_s, ep, ts.to_string(), "POWER_LOSS".to_string()));
                        }
                    }
                }
                _ => {}
            }
            prev_ep = ep;
        }

        // 末尾の未クローズIDLE_START（まだIDLE_RESUMEが来ていない進行中セッション）は
        // ここでは完了扱いにしない。以前はここでDEVICE_ONを見つけて仮クローズしていたが、
        // 一晩の間に混じる無関係なDEVICE_ON（タブレット画面が一瞬ついただけ等）を
        // 拾って実際の睡眠時間より大幅に短いセッションを捏造してしまうバグがあった
        // （DEVICE_ON単体は「起きていた証拠」にならない、というAPP_USAGE方式への
        // 移行時の設計方針と矛盾していた）。進行中セッションの表示は
        // current_sleep_start()（暫定睡眠時間）が別途担当する。
    }

    // APP_USAGE区間の統合・フィルタ（画面ロック→即再開のような細切れ検知対策と、
    // 通知を一瞬見ただけ等のノイズ除去）。詳細はコメント参照。
    let app_usage_pairs = coalesce_and_filter_app_usage(app_usage_pairs);

    // Pass 2 ─────────────────────────────────────────────────────────────────────
    let mut sessions: Vec<Session> = Vec::new();

    for (idle_start, idle_start_ts, idle_end, idle_end_ts) in &idle_pairs {
        // OUT・APP_USAGEいずれも「起きていた証拠」として全く同じ除外ギャップ扱いにする。
        let mut gaps: Vec<(i64, String, i64, String)> = out_pairs.iter()
            .chain(app_usage_pairs.iter())
            .filter_map(|(os, os_ts, oe, oe_ts)| {
                let s = (*os).max(*idle_start);
                let e = (*oe).min(*idle_end);
                if e > s {
                    let sts = if *os >= *idle_start { os_ts.clone() } else { idle_start_ts.clone() };
                    let ets = if *oe <= *idle_end    { oe_ts.clone()  } else { idle_end_ts.clone()   };
                    Some((s, sts, e, ets))
                } else {
                    None
                }
            })
            .collect();
        gaps.sort_by_key(|(s, _, _, _)| *s);

        let mut cur_ep  = *idle_start;
        let mut cur_ts  = idle_start_ts.clone();

        macro_rules! emit_seg {
            ($end_ep:expr, $end_ts:expr) => {{
                let dur = $end_ep - cur_ep;
                if dur >= min_sleep_secs {
                    sessions.push(Session {
                        excluded: excluded_dates.contains(cur_ts.get(..10).unwrap_or("")),
                        start: cur_ts.clone(),
                        end: $end_ts.to_string(),
                        duration_hours: dur as f64 / 3600.0,
                        session_type: "IDLE".to_string(),
                    });
                }
            }};
        }

        // ギャップを時系列順に処理し、cur_epより後ろにあるギャップだけを切り出す。
        // 重複・隣接するギャップ（OUTとAPP_USAGEが重なる等）は自然に併合される
        // （cur_epをmaxで前進させるだけなので、二重に切り出されることはない）。
        for (gs, gs_ts, ge, ge_ts) in &gaps {
            if *gs > cur_ep {
                emit_seg!(*gs, gs_ts.as_str());
            }
            if *ge > cur_ep {
                cur_ep = *ge;
                cur_ts = ge_ts.clone();
            }
        }

        // Close the final segment
        emit_seg!(*idle_end, idle_end_ts.as_str());
    }

    // Append POWER sessions and sort chronologically
    for (pep, pts, eep, ets, ptype) in power_sessions {
        let excl = excluded_dates.contains(pts.get(..10).unwrap_or(""));
        sessions.push(Session {
            start: pts,
            end: ets,
            duration_hours: (eep - pep) as f64 / 3600.0,
            session_type: ptype,
            excluded: excl,
        });
    }
    sessions.sort_by(|a, b| a.start.cmp(&b.start));

    // Filter out soft-deleted sessions.
    sessions.retain(|s| !deleted_starts.contains(&s.start));

    // Merge manual sessions from sleep_manual.txt (supports MANUAL_DELETED soft-delete)
    if let Some(manual_raw) = manual_raw {
        // First pass: collect soft-deleted start timestamps
        let mut manual_deleted: std::collections::HashSet<String> = std::collections::HashSet::new();
        for line in manual_raw.lines() {
            let line = line.trim().trim_start_matches('\u{FEFF}');
            if let Some(c) = line.find(',') {
                if &line[c+1..] == "MANUAL_DELETED" {
                    manual_deleted.insert(line[..c].to_string());
                }
            }
        }
        // Second pass: add non-deleted sessions
        for line in manual_raw.lines() {
            let line = line.trim().trim_start_matches('\u{FEFF}');
            if line.is_empty() { continue; }
            if let Some(c) = line.find(',') {
                let start = &line[..c];
                let end   = &line[c+1..];
                if end == "MANUAL_DELETED" { continue; }
                if manual_deleted.contains(start) { continue; }
                if let (Some(sep), Some(eep)) = (ts_to_epoch(start), ts_to_epoch(end)) {
                    let dur = eep - sep;
                    if dur > 0 {
                        sessions.push(Session {
                            excluded: excluded_dates.contains(start.get(..10).unwrap_or("")),
                            start: start.to_string(),
                            end: end.to_string(),
                            duration_hours: dur as f64 / 3600.0,
                            session_type: "MANUAL".to_string(),
                        });
                    }
                }
            }
        }
        sessions.sort_by(|a, b| a.start.cmp(&b.start));
    }

    (sessions, event_count)
}

pub fn get_sessions() -> Result<Vec<Session>, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let mtime_of = |name: &str| -> std::time::SystemTime {
        crate::data_dir().join(name).metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    };
    // Cache is valid only when both source files are unchanged.
    let current_mtime = mtime_of("sleep_events.txt").max(mtime_of("sleep_manual.txt"));

    let mut cache = SESSION_CACHE.lock().unwrap();
    if let Some(c) = cache.as_ref() {
        if c.mtime == current_mtime {
            eprintln!("{} parse_sessions #{}: cache HIT", TAG, n);
            return Ok(c.sessions.clone());
        }
    }

    let sessions = parse_sessions_rust()?;
    *cache = Some(SessionCache { sessions: sessions.clone(), mtime: current_mtime });
    Ok(sessions)
}

// 手動追加する睡眠時間が既存のセッションと重なっていないか確認する。
// タイムスタンプは "YYYY-MM-DD HH:MM:SS" 固定長のため文字列比較がそのまま
// 時刻比較として使える。
fn overlaps_existing(start: &str, end: &str) -> Option<(String, String)> {
    let sessions = get_sessions().unwrap_or_default();
    sessions.into_iter()
        .find(|s| start < s.end.as_str() && end > s.start.as_str())
        .map(|s| (s.start, s.end))
}

pub fn add_session(start: String, end: String) -> Result<(), String> {
    if let Some((os, oe)) = overlaps_existing(&start, &end) {
        return Err(format!("既存の睡眠記録（{} → {}）と重なっています", os, oe));
    }
    eprintln!("{} add_session: {} → {}", TAG, start, end);
    let path = crate::data_dir().join("sleep_manual.txt");
    let line = format!("{},{}\n", start, end);
    let mut f = OpenOptions::new().create(true).append(true).open(&path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    *SESSION_CACHE.lock().unwrap() = None;
    let p = path.clone();
    std::thread::spawn(move || { super::cloud::auto_backup_manual(&p); });
    Ok(())
}

pub fn delete_session(start: String, _end: String) -> Result<(), String> {
    eprintln!("{} delete_session: {}", TAG, start);

    // Check if this is a manual session (start exists in sleep_manual.txt as non-deleted entry).
    let manual_path = crate::data_dir().join("sleep_manual.txt");
    if manual_path.exists() {
        let content = std::fs::read_to_string(&manual_path).map_err(|e| e.to_string())?;
        let is_manual = content.lines().any(|l| {
            if let Some(c) = l.find(',') { &l[..c] == start.as_str() && &l[c+1..] != "MANUAL_DELETED" }
            else { false }
        });
        if is_manual {
            // Soft-delete: append MANUAL_DELETED marker so deletion survives Drive sync.
            let marker = format!("{},MANUAL_DELETED\n", start);
            let mut f = OpenOptions::new().create(true).append(true).open(&manual_path)
                .map_err(|e| e.to_string())?;
            f.write_all(marker.as_bytes()).map_err(|e| e.to_string())?;
            eprintln!("{} delete_session: MANUAL_DELETED appended to sleep_manual.txt", TAG);
            sort_manual_file(&manual_path)?;
            *SESSION_CACHE.lock().unwrap() = None;
            let p = manual_path.clone();
            std::thread::spawn(move || { super::cloud::auto_backup_manual(&p); });
            return Ok(());
        }
    }

    // Auto-detected session: soft-delete via SESSION_DELETED marker so the deletion
    // survives sync (the original events remain in the file but are filtered at parse time).
    let events_path = crate::data_dir().join("sleep_events.txt");
    if !events_path.exists() {
        return Err("sleep_events.txt not found".to_string());
    }
    let marker = format!("{},SESSION_DELETED\n", start);
    let mut f = OpenOptions::new().create(true).append(true).open(&events_path)
        .map_err(|e| e.to_string())?;
    f.write_all(marker.as_bytes()).map_err(|e| e.to_string())?;
    sort_events_file(&events_path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    let ep = events_path.clone();
    std::thread::spawn(move || { super::cloud::auto_backup_after_event(&ep); });
    Ok(())
}

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
// バックグラウンドスレッドを持てないため、android_bg.rsのフォアグラウンド中
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

// ローカルの sleep_events.txt / sleep_manual.txt を両方とも空にする（実際には
// HARD_RESETマーカー1行だけを残す。理由はHARD_RESET_TAGのドキュメント参照）。
// クラウド（Drive・スプレッドシート）は消さない。
pub fn clear_all_data() -> Result<(), String> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let reset_line = format!("{},{}\n", now, HARD_RESET_TAG);
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, &reset_line).map_err(|e| e.to_string())?;
    let manual_path = crate::data_dir().join("sleep_manual.txt");
    std::fs::write(&manual_path, &reset_line).map_err(|e| e.to_string())?;
    *SESSION_CACHE.lock().unwrap() = None;
    Ok(())
}

// ファイルを時刻順に走査し、START系イベントで開き、対応するEND系イベントで閉じる。
// 現在進行中で閉じていない IDLE_START / OUT_START のタイムスタンプを検出する
// （compact_data と current_sleep_start の両方から使う共通ロジック）。
fn detect_open_idle_and_out(raw: &str) -> (Option<String>, Option<String>) {
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

    let mut lines: Vec<String> = Vec::with_capacity(sessions.len() * 2 + 2);
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
    // HARD_RESETマーカー（詳細はHARD_RESET_TAGのドキュメント参照）: 圧縮によって
    // 消えた生イベント（APP_USAGE/DEVICE_ON等の細切れ）が、もう一方の端末との
    // 次回同期でunionマージにより復活しないようにする。
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let reset_line = format!("{},{}", now, HARD_RESET_TAG);
    lines.push(reset_line.clone());
    lines.sort();
    let content = lines.join("\n") + "\n";
    let manual_content = reset_line + "\n";

    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    std::fs::write(&events_path, &content).map_err(|e| e.to_string())?;
    std::fs::write(&manual_path, &manual_content).map_err(|e| e.to_string())?;
    drop(_lock);

    *SESSION_CACHE.lock().unwrap() = None;
    eprintln!("{} compact_data: {} sessions → {} lines ({:.1}KB)", TAG, sessions.len(), lines.len(), content.len() as f64 / 1024.0);
    Ok(content)
}

pub fn export_csv(sessions: &[Session]) -> String {
    let mut out = String::from("就寝時刻,起床時刻,睡眠時間(時間),種別\n");
    for s in sessions {
        out.push_str(&format!("{},{},{:.4},{}\n", s.start, s.end, s.duration_hours, s.session_type));
    }
    out
}

pub fn write_csv_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

pub fn import_csv(csv: String) -> Result<usize, String> {
    let path = crate::data_dir().join("sleep_events.txt");
    let existing = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();

    let mut added = 0usize;
    for line in csv.lines().skip(1) {  // skip header
        let cols: Vec<&str> = line.splitn(4, ',').collect();
        if cols.len() < 2 { continue; }
        let start = cols[0].trim();
        let end = cols[1].trim();
        if start.len() < 19 || end.len() < 19 { continue; }
        lines.push(format!("{},IDLE_START", start));
        lines.push(format!("{},IDLE_RESUME", end));
        added += 1;
    }
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    sort_events_file(&path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    eprintln!("{} import_csv: {} sessions added", TAG, added);
    Ok(added)
}

#[cfg(test)]
mod events_tests;
