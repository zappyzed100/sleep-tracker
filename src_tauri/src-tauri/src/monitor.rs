use chrono::{Duration as CDur, Local};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

// ── Windows API ───────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    #[repr(C)]
    pub struct LASTINPUTINFO {
        pub cb_size: u32,
        pub dw_time: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn GetLastInputInfo(p: *mut LASTINPUTINFO) -> i32;
        pub fn GetTickCount() -> u32;
    }

    pub fn idle_ms() -> u64 {
        unsafe {
            let mut lii = LASTINPUTINFO {
                cb_size: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dw_time: 0,
            };
            if GetLastInputInfo(&mut lii) != 0 {
                GetTickCount().wrapping_sub(lii.dw_time) as u64
            } else {
                0
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn idle_ms() -> u64 { win::idle_ms() }
#[cfg(not(target_os = "windows"))]
fn idle_ms() -> u64 { 0 }

fn idle_secs() -> u64 { idle_ms() / 1000 }

// ── Timestamp helpers ─────────────────────────────────────────────────────────

fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// Returns local timestamp for (now - secs_ago), rounded to seconds
fn ago_str(secs_ago: u64) -> String {
    (Local::now() - CDur::seconds(secs_ago as i64))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

// ── File helpers ──────────────────────────────────────────────────────────────

fn append_event(path: &PathBuf, ts: &str, kind: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{},{}", ts, kind);
    }
}

fn write_heartbeat(path: &PathBuf, idle_ms_val: u64) {
    let _ = fs::write(path, format!("{},{}\n", now_str(), idle_ms_val));
}

// ── Config ────────────────────────────────────────────────────────────────────

fn read_threshold_secs(config_path: &PathBuf) -> u64 {
    let minutes: u64 = fs::read_to_string(config_path).ok()
        .and_then(|s| {
            let needle = "\"idle_threshold_minutes\"";
            let pos = s.find(needle)?;
            let after = &s[pos + needle.len()..];
            let colon = after.find(':')?;
            let v = after[colon + 1..].trim_start();
            let end = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
            v[..end].parse().ok()
        })
        .unwrap_or(60);
    minutes.max(1) * 60
}

// ── Monitor loop ──────────────────────────────────────────────────────────────

pub fn start(data_dir: PathBuf, config_path: PathBuf) {
    thread::Builder::new()
        .name("sleep-monitor".into())
        .spawn(move || run(data_dir, config_path))
        .expect("failed to spawn monitor thread");
}

fn run(data_dir: PathBuf, config_path: PathBuf) {
    let events_path    = data_dir.join("sleep_events.txt");
    let heartbeat_path = data_dir.join("sleep_heartbeat.txt");
    let pause_flag     = data_dir.join("monitor_paused");

    const POLL:        Duration = Duration::from_secs(5);
    const HB_INTERVAL: Duration = Duration::from_secs(30);
    // User is considered "awake" once idle drops below this
    const WAKE_SECS: u64 = 300; // 5 minutes

    append_event(&events_path, &now_str(), "STARTUP");

    let mut sleeping     = false;
    let mut threshold    = read_threshold_secs(&config_path);
    let mut last_hb      = Instant::now();
    let mut last_tick    = Instant::now();
    let mut cfg_counter  = 0u32;

    // If already idle at startup beyond the threshold, enter sleeping state
    // immediately so we don't miss a session in progress.
    {
        let current_idle = idle_secs();
        if current_idle >= threshold {
            let start_ts = ago_str(current_idle);
            append_event(&events_path, &start_ts, "IDLE_START");
            sleeping = true;
        }
    }

    loop {
        thread::sleep(POLL);

        let now = Instant::now();

        // ── Suspend / resume detection ────────────────────────────────────────
        // If sleep() returned far later than POLL, the system likely suspended.
        let actual_elapsed = now.duration_since(last_tick);
        if actual_elapsed > POLL + Duration::from_secs(60) {
            // System resumed from suspend
            if sleeping {
                append_event(&events_path, &now_str(), "RESUME");
                sleeping = false;
            } else {
                // Gap with no session: treat as untracked power-loss sleep
                // parse_sessions.exe detects this via heartbeat
            }
        }
        last_tick = now;

        // ── Reload config periodically (~5 min) ───────────────────────────────
        cfg_counter += 1;
        if cfg_counter >= 60 {
            threshold = read_threshold_secs(&config_path);
            cfg_counter = 0;
        }

        let idle   = idle_secs();
        let idle_m = idle_ms();

        // ── Heartbeat (always, even when paused) ──────────────────────────────
        if last_hb.elapsed() >= HB_INTERVAL {
            write_heartbeat(&heartbeat_path, idle_m);
            last_hb = Instant::now();
        }

        // ── Skip detection if paused ──────────────────────────────────────────
        if pause_flag.exists() {
            continue;
        }

        // ── State machine ─────────────────────────────────────────────────────
        if !sleeping && idle >= threshold {
            let start_ts = ago_str(idle);
            append_event(&events_path, &start_ts, "IDLE_START");
            sleeping = true;
        } else if sleeping && idle < WAKE_SECS {
            append_event(&events_path, &now_str(), "IDLE_RESUME");
            sleeping = false;
        }
    }
}
