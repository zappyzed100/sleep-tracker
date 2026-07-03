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
//!        `maybe_auto_backup`, `clear_backups`,
//!        `export_csv`, `write_csv_file`, `import_csv`,
//!        `is_out_from_content`, `apply_mobile_event_line`

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

const TAG: &str = "[events]";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub start: String,
    pub end: String,
    #[serde(rename = "duration")]
    pub duration_hours: f64,
    #[serde(rename = "type")]
    pub session_type: String,
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
                "OUT_END" | "IN_HOUSE" | "DEVICE_ON" => out = false,
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
    let need_in_house = event_type == "DEVICE_ON" && is_out_from_content(&existing);

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

pub fn parse_sessions_rust() -> Result<Vec<Session>, String> {
    use chrono::NaiveDateTime;

    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let events_path   = crate::data_dir().join("sleep_events.txt");
    let heartbeat_path = crate::data_dir().join("sleep_heartbeat.txt");
    let min_sleep_secs = crate::THRESHOLD_SECS.load(Ordering::Relaxed) as i64;

    let ts_to_epoch = |s: &str| -> Option<i64> {
        let ndt = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
        Some(ndt.and_utc().timestamp())
    };

    let epoch_to_ts = |ep: i64| -> String {
        use chrono::DateTime;
        let dt = DateTime::from_timestamp(ep, 0).map(|d| d.naive_utc());
        dt.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default()
    };

    // Heartbeat (for POWER_LOSS start-time correction)
    let mut hb_epoch: i64 = 0;
    let mut hb_idle_ms: i64 = 0;
    if let Ok(txt) = std::fs::read_to_string(&heartbeat_path) {
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

    // Events
    if !events_path.exists() {
        eprintln!("{} parse_sessions #{}: cache MISS — no file", TAG, n);
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&events_path).map_err(|e| e.to_string())?;
    let kb = raw.len() as f64 / 1024.0;

    let t0 = std::time::Instant::now();

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
    //   IDLE pair : IDLE_START → 次の IDLE_RESUME
    //   OUT pair  : OUT_START  → 次の OUT_END
    //
    // Pass 1: 閉じたペアと DEVICE_ON を収集する。
    // Pass 2: 各 IDLE ペア内から OUT 期間と DEVICE_ON で区切った細切れを作り、
    //         規定時間以上のものを睡眠セッションとして記録する。

    // Pass 1 ─────────────────────────────────────────────────────────────────────
    // (start_ep, start_ts, end_ep, end_ts)
    let mut idle_pairs: Vec<(i64, String, i64, String)> = Vec::new();
    let mut out_pairs:  Vec<(i64, String, i64, String)> = Vec::new();
    let mut device_ons: Vec<(i64, String)>              = Vec::new();
    // POWER session tracking: (start_ep, start_ts, end_ep, end_ts, type)
    let mut power_sessions: Vec<(i64, String, i64, String, String)> = Vec::new();
    {
        let mut idle_pend:  Option<(i64, String)> = None;
        let mut out_pend:   Option<(i64, String)> = None;
        let mut power_pend: Option<(i64, String)> = None;
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
                "DEVICE_ON"   => { device_ons.push((ep, ts.to_string())); }
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

        // ── Close unclosed IDLE_START with the first subsequent DEVICE_ON ──────────
        // 起床時にPCを操作する前にAndroidを触った場合、IDLE_RESUME が記録されず
        // DEVICE_ON しか終了イベントが存在しない。この未閉じペアを DEVICE_ON で閉じる。
        if let Some((start_ep, start_ts)) = idle_pend {
            if let Some(&(dep, ref dts)) = device_ons.iter().find(|(d, _)| *d > start_ep) {
                eprintln!("{} parse_sessions: unclosed IDLE_START closed by DEVICE_ON ({} → {})", TAG, start_ts, dts);
                idle_pairs.push((start_ep, start_ts, dep, dts.clone()));
            }
        }
    }

    // Pass 2 ─────────────────────────────────────────────────────────────────────
    let mut sessions: Vec<Session> = Vec::new();

    for (idle_start, idle_start_ts, idle_end, idle_end_ts) in &idle_pairs {
        // OUT gaps that overlap this IDLE window (clip to window boundaries)
        let mut gaps: Vec<(i64, String, i64, String)> = out_pairs.iter()
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

        // DEVICE_ON split points within the IDLE window
        let dins: Vec<(i64, String)> = device_ons.iter()
            .filter(|(d, _)| *d > *idle_start && *d < *idle_end)
            .cloned()
            .collect();

        // Merge gap boundaries and DEVICE_ON into a single sorted timeline.
        // Same-epoch tie-breaking: GapEnd(0) < DeviceOn(1) < GapStart(2)
        // so that gaps close before splits, and splits happen before new gaps open.
        let mut tl: Vec<(i64, u8, String)> = Vec::new();
        for (gs, gs_ts, ge, ge_ts) in &gaps {
            tl.push((*gs, 2, gs_ts.clone()));
            tl.push((*ge, 0, ge_ts.clone()));
        }
        for (dep, dts) in &dins {
            tl.push((*dep, 1, dts.clone()));
        }
        tl.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut cur_ep  = *idle_start;
        let mut cur_ts  = idle_start_ts.clone();
        let mut in_gap  = false;

        macro_rules! emit_seg {
            ($end_ep:expr, $end_ts:expr) => {{
                let dur = $end_ep - cur_ep;
                if dur >= min_sleep_secs {
                    sessions.push(Session {
                        start: cur_ts.clone(),
                        end: $end_ts.to_string(),
                        duration_hours: dur as f64 / 3600.0,
                        session_type: "IDLE".to_string(),
                    });
                }
            }};
        }

        for (ep, kind, ts) in &tl {
            match kind {
                2 => { // GapStart: close current segment, enter gap
                    if !in_gap { emit_seg!(*ep, ts.as_str()); }
                    in_gap = true;
                }
                0 => { // GapEnd: leave gap, resume from here
                    in_gap = false;
                    cur_ep = *ep;
                    cur_ts = ts.clone();
                }
                1 => { // DeviceOn: split point (only outside a gap)
                    if !in_gap {
                        emit_seg!(*ep, ts.as_str());
                        cur_ep = *ep;
                        cur_ts = ts.clone();
                    }
                }
                _ => {}
            }
        }

        // Close the final segment
        if !in_gap { emit_seg!(*idle_end, idle_end_ts.as_str()); }
    }

    // Append POWER sessions and sort chronologically
    for (pep, pts, eep, ets, ptype) in power_sessions {
        sessions.push(Session {
            start: pts,
            end: ets,
            duration_hours: (eep - pep) as f64 / 3600.0,
            session_type: ptype,
        });
    }
    sessions.sort_by(|a, b| a.start.cmp(&b.start));

    // Filter out soft-deleted sessions.
    if !deleted_starts.is_empty() {
        let before = sessions.len();
        sessions.retain(|s| !deleted_starts.contains(&s.start));
        let removed = before - sessions.len();
        if removed > 0 {
            eprintln!("{} parse_sessions #{}: {} sessions filtered by SESSION_DELETED", TAG, n, removed);
        }
    }

    // Merge manual sessions from sleep_manual.txt (supports MANUAL_DELETED soft-delete)
    let manual_path = crate::data_dir().join("sleep_manual.txt");
    if manual_path.exists() {
        if let Ok(manual_raw) = std::fs::read_to_string(&manual_path) {
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
    }

    let ms = t0.elapsed().as_millis();
    eprintln!("{} parse_sessions #{}: cache MISS — {} events → {} sessions ({:.1}KB)  (+{}ms)",
        TAG, n, event_count, sessions.len(), kb, ms);

    Ok(sessions)
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

// Android: write DEVICE_ON (+ IN_HOUSE if out-state) when user opens the app.
pub fn record_device_on() {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;

    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    use chrono::Local;
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let path = crate::data_dir().join("sleep_events.txt");
    let existing = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    let lines_before = existing.lines().filter(|l| !l.trim().is_empty()).count();
    let is_out = is_out_from_content(&existing);
    eprintln!("{} record_device_on #{}: ts={} is_out={} lines_before={}", TAG, n, ts, is_out, lines_before);

    // IN_HOUSE と DEVICE_ON を1回のファイルオープンで書き込む（競合防止）
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        if is_out {
            let _ = writeln!(f, "{},IN_HOUSE", ts);
            eprintln!("{} record_device_on #{}: wrote IN_HOUSE", TAG, n);
        }
        let _ = writeln!(f, "{},DEVICE_ON", ts);
    }
    let _ = std::fs::write(crate::data_dir().join("device_heartbeat.txt"), format!("{}\n", ts));
    *SESSION_CACHE.lock().unwrap() = None;

    let lines_after = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines().filter(|l| !l.trim().is_empty()).count();
    eprintln!("{} record_device_on #{}: done lines_after={} (+{})", TAG, n, lines_after, lines_after.saturating_sub(lines_before));

    if n > 10 {
        eprintln!("{} record_device_on #{}: WARN called {} times — possible visibilitychange loop", TAG, n, n);
    }
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
// data/backups/ に書き出す。前回バックアップ時刻は data/last_auto_backup.txt に
// 保持し、アプリ再起動をまたいでも正しく約1日おきになるようにする。
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

    let backups_dir = data_dir.join("backups");
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

// data/backups/ 以下を全削除する（手動バックアップ・自動バックアップとも対象）。
// 現在のsleep_events.txt/sleep_manual.txt自体には触れない。
pub fn clear_backups(data_dir: &std::path::Path) -> Result<(), String> {
    let backups_dir = data_dir.join("backups");
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

// ローカルの sleep_events.txt / sleep_manual.txt を両方とも空にする。
// クラウド（Drive・スプレッドシート）は消さない。
pub fn clear_all_data() -> Result<(), String> {
    let path = crate::data_dir().join("sleep_events.txt");
    std::fs::write(&path, "").map_err(|e| e.to_string())?;
    let manual_path = crate::data_dir().join("sleep_manual.txt");
    std::fs::write(&manual_path, "").map_err(|e| e.to_string())?;
    *SESSION_CACHE.lock().unwrap() = None;
    Ok(())
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

    // 現在進行中で閉じていないIDLE_START/OUT_STARTのタイムスタンプを検出する。
    // ファイルを時刻順に走査し、START系イベントで開き、対応するEND系イベントで閉じる。
    let raw = std::fs::read_to_string(&events_path).unwrap_or_default();
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
    lines.sort();
    let content = if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" };

    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    std::fs::write(&events_path, &content).map_err(|e| e.to_string())?;
    std::fs::write(&manual_path, "").map_err(|e| e.to_string())?;
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
