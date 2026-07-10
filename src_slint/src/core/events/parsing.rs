//! parsing.rs — sleep_events.txt のイベント行→睡眠セッション変換ステートマシン
//!
//! 役割 : 生のイベント行（IDLE_START/IDLE_RESUME等）を時系列の状態機械で処理し、
//!        Session一覧を組み立てる。ファイルソート・モバイルイベント取り込み・
//!        SESSION_CACHEを介したキャッシュ付き取得もここで担当する。
//!
//! 依存 : super::{Session, SessionCache, SESSION_CACHE, EVENTS_FILE_LOCK, TAG},
//!        super::excluded::excluded_dates_from_content, crate::core::utils::sleep_day

use std::io::Write;
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Session, SessionCache, SESSION_CACHE, EVENTS_FILE_LOCK, TAG};
use super::excluded::excluded_dates_from_content;

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
pub(super) fn coalesce_and_filter_app_usage(mut pairs: Vec<(i64, String, i64, String)>) -> Vec<(i64, String, i64, String)> {
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

// セッション開始時刻(ts)の「睡眠日」（暦日ではなく午前4時境界、crate::core::utils::sleep_day
// 参照）が計測対象外としてマークされているかを判定する。生のタイムスタンプの暦日
// （ts[..10]）で比較すると、日付境界をまたぐ深夜開始のセッション（例:
// 6/30 01:14開始 → 睡眠日は6/29）が、6/29を対象外にしても除外されない
// バグになるため、必ず睡眠日ベースで比較すること。
fn is_excluded_at(ts: &str, excluded_dates: &std::collections::HashSet<String>) -> bool {
    let Some(ndt) = chrono::NaiveDateTime::parse_from_str(ts.trim(), "%Y-%m-%d %H:%M:%S").ok() else {
        return excluded_dates.contains(ts.get(..10).unwrap_or(""));
    };
    excluded_dates.contains(&crate::core::utils::sleep_day(ndt).format("%Y-%m-%d").to_string())
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
pub(super) fn parse_sessions_from_str(
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
    // だけでは起きていた証拠にならないため（詳細はplatform/android/bg.rsのコメント参照）。
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
                        excluded: is_excluded_at(&cur_ts, &excluded_dates),
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
        let excl = is_excluded_at(&pts, &excluded_dates);
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
                            excluded: is_excluded_at(start, &excluded_dates),
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
