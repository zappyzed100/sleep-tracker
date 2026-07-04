//! monitor.rs — バックグラウンドアイドル監視スレッド（Windowsデスクトップ専用）
//!
//! 役割 : キーボード/マウス/ゲームパッドのアイドル時間を定期ポーリングし、
//!        閾値超過で IDLE_START、入力再開で IDLE_RESUME を sleep_events.txt に書き込む。
//!        システムサスペンド/レジュームも検出する。
//!        Tauri版 src-tauri/src/monitor.rs の移植。
//!        `app.emit("sleep-session-recorded")` は Slint の `invoke_from_event_loop` +
//!        ウィンドウコールバック呼び出しに置き換えている。
//!
//! 依存 : crate::THRESHOLD_SECS（閾値の共有アトミック）、crate::ui::sync_status
//! 公開 : `start(data_dir: PathBuf, weak: slint::Weak<MainWindow>, on_session_recorded: impl Fn() + Send + 'static)`

use crate::ui::sync_status;
use crate::MainWindow;
use chrono::{Duration as CDur, Local};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const TAG: &str = "[monitor]";

// ── Windows API ───────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    // ── Keyboard / mouse idle (GetLastInputInfo) ──────────────────────────────
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

    // ── XInput gamepad (loaded dynamically so missing DLL won't crash) ────────
    #[repr(C)]
    struct XINPUT_GAMEPAD {
        w_buttons:       u16,
        b_left_trigger:  u8,
        b_right_trigger: u8,
        s_thumb_lx:      i16,
        s_thumb_ly:      i16,
        s_thumb_rx:      i16,
        s_thumb_ry:      i16,
    }

    #[repr(C)]
    struct XINPUT_STATE {
        dw_packet_number: u32,
        gamepad:          XINPUT_GAMEPAD,
    }

    type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> usize;
        fn GetProcAddress(module: usize, name: *const u8) -> usize;
    }

    fn xinput_fn() -> Option<XInputGetStateFn> {
        static ADDR: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let addr = *ADDR.get_or_init(|| unsafe {
            for dll in &[b"xinput1_4.dll\0" as &[u8], b"xinput1_3.dll\0"] {
                let h = LoadLibraryA(dll.as_ptr());
                if h != 0 {
                    let f = GetProcAddress(h, b"XInputGetState\0".as_ptr());
                    if f != 0 { return f; }
                }
            }
            0
        });
        if addr == 0 { None } else { Some(unsafe { std::mem::transmute(addr) }) }
    }

    /// Returns `(any_connected, any_active)`.
    /// `any_connected` = at least one controller is plugged in.
    /// `any_active`    = a connected controller has button/trigger/stick input.
    pub fn poll_gamepad() -> (bool, bool) {
        let Some(xinput) = xinput_fn() else { return (false, false) };

        const TRIGGER_THRESHOLD: u8  = 30;
        const THUMB_DEADZONE:    i16 = 8000;

        let mut any_connected = false;
        let mut any_active    = false;

        for i in 0..4u32 {
            let mut state = XINPUT_STATE {
                dw_packet_number: 0,
                gamepad: XINPUT_GAMEPAD {
                    w_buttons: 0, b_left_trigger: 0, b_right_trigger: 0,
                    s_thumb_lx: 0, s_thumb_ly: 0, s_thumb_rx: 0, s_thumb_ry: 0,
                },
            };
            if unsafe { xinput(i, &mut state) } != 0 { continue; } // not connected
            any_connected = true;
            let gp = &state.gamepad;
            if gp.w_buttons != 0
                || gp.b_left_trigger  > TRIGGER_THRESHOLD
                || gp.b_right_trigger > TRIGGER_THRESHOLD
                || gp.s_thumb_lx.abs() > THUMB_DEADZONE
                || gp.s_thumb_ly.abs() > THUMB_DEADZONE
                || gp.s_thumb_rx.abs() > THUMB_DEADZONE
                || gp.s_thumb_ry.abs() > THUMB_DEADZONE
            {
                any_active = true;
            }
        }
        (any_connected, any_active)
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
    let _lock = crate::core::events::EVENTS_FILE_LOCK.lock().unwrap();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{},{}", ts, kind);
    }
}

fn write_heartbeat(path: &PathBuf, idle_ms_val: u64) {
    let _ = fs::write(path, format!("{},{}\n", now_str(), idle_ms_val));
}

// ── Out-state helpers ─────────────────────────────────────────────────────────

fn is_currently_out(events_path: &PathBuf) -> bool {
    let Ok(content) = fs::read_to_string(events_path) else { return false };
    let mut out = false;
    for line in content.lines() {
        if let Some(c) = line.trim().find(',') {
            match &line.trim()[c + 1..] {
                "OUT_START" => out = true,
                "OUT_END" | "IN_HOUSE" => out = false,
                _ => {}
            }
        }
    }
    out
}

// Write IN_HOUSE at ts if the user is currently marked as out.
fn maybe_in_house(events_path: &PathBuf, ts: &str) {
    if is_currently_out(events_path) {
        append_event(events_path, ts, "IN_HOUSE");
        eprintln!("{} IN_HOUSE: written (was out)", TAG);
    }
}

// 日次ローカル自動バックアップ本体は core::events::maybe_auto_backup に共通化されている
// （PC・Android両方から呼べるようにするため）。

// ── Monitor loop ──────────────────────────────────────────────────────────────

/// `on_session_recorded` は IDLE_RESUME（起床検知）のたびに UI スレッドで呼ばれる。
/// Slint 側では `slint::invoke_from_event_loop` でラップしたクロージャを渡すこと。
/// `weak` は同期中インジケーター(sync_status)の更新に使う。
pub fn start(
    data_dir: PathBuf,
    weak: slint::Weak<MainWindow>,
    on_session_recorded: impl Fn() + Send + 'static,
) {
    thread::Builder::new()
        .name("sleep-monitor".into())
        .spawn(move || run(data_dir, weak, on_session_recorded))
        .expect("failed to spawn monitor thread");
}

fn run(data_dir: PathBuf, weak: slint::Weak<MainWindow>, on_session_recorded: impl Fn() + Send + 'static) {
    let events_path    = data_dir.join("sleep_events.txt");
    let heartbeat_path = data_dir.join("sleep_heartbeat.txt");
    let pause_flag     = data_dir.join("monitor_paused");

    eprintln!("{} start: data_dir={:?}", TAG, data_dir);

    const POLL:        Duration = Duration::from_secs(5);
    const HB_INTERVAL: Duration = Duration::from_secs(30);
    const PULL_INTERVAL: Duration = Duration::from_secs(60); // Drive/Sheet からモバイルイベントを定期pull
    // 実際に「7日経過したか」はdata/last_auto_backup.txtで判定するので、ここは
    // その判定を行う頻度（何度もディスクI/Oしないよう1時間おき）。
    const BACKUP_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
    // User is considered "awake" once idle drops below 60s.
    // Must be < any realistic threshold to prevent start/resume oscillation.
    const WAKE_SECS: u64 = 60;

    maybe_in_house(&events_path, &now_str());
    crate::core::events::maybe_auto_backup(&data_dir);

    let mut sleeping              = false;
    let mut threshold             = crate::THRESHOLD_SECS.load(std::sync::atomic::Ordering::Relaxed);
    let mut last_hb               = Instant::now();
    let mut last_tick             = Instant::now();
    let mut last_pull             = Instant::now();
    let mut last_backup_check     = Instant::now();
    let mut gamepad_last_active   = Instant::now();

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
            eprintln!("{} RESUME: system woke from suspend", TAG);
            // System resumed from suspend
            if sleeping {
                maybe_in_house(&events_path, &now_str());
                append_event(&events_path, &now_str(), "RESUME");
                sleeping = false;
            } else {
                // Gap with no session: treat as untracked power-loss sleep
                // (detected later via heartbeat during parse)
            }
        }
        last_tick = now;

        threshold = crate::THRESHOLD_SECS.load(std::sync::atomic::Ordering::Relaxed);

        let mut idle_m = idle_ms();

        // ── Gamepad: if connected use min(kb/mouse idle, gamepad idle) ─────────
        #[cfg(target_os = "windows")]
        {
            let (connected, active) = win::poll_gamepad();
            if active { gamepad_last_active = Instant::now(); }
            if connected {
                let gp_ms = gamepad_last_active.elapsed().as_millis() as u64;
                idle_m = idle_m.min(gp_ms);
            }
        }

        let idle = idle_m / 1000;

        // ── Heartbeat (always, even when paused) ──────────────────────────────
        if last_hb.elapsed() >= HB_INTERVAL {
            write_heartbeat(&heartbeat_path, idle_m);
            last_hb = Instant::now();
        }

        // ── 日次自動バックアップ判定（常時、監視中断中でも実行） ─────────────────
        if last_backup_check.elapsed() >= BACKUP_CHECK_INTERVAL {
            last_backup_check = Instant::now();
            let dd = data_dir.clone();
            thread::spawn(move || crate::core::events::maybe_auto_backup(&dd));
        }

        // ── Skip detection if paused ──────────────────────────────────────────
        if pause_flag.exists() {
            continue;
        }

        // ── Periodic Drive/Sheet pull + push (双方向で毎回同期する) ──────────────
        // pullだけだとIDLE_START時の一回限りのpush（auto_backup_after_event）が
        // 何らかの理由（ネットワーク瞬断等）で失敗した場合にリトライされず、
        // タブレット側が進行中セッションを見られないままになる。そのため周期処理でも
        // 毎回ローカルの内容をDriveへpushし直し、初回push失敗からの自動復旧を保証する。
        if last_pull.elapsed() >= PULL_INTERVAL {
            last_pull = Instant::now();
            let ep = events_path.clone();
            let w = weak.clone();
            sync_status::begin(&w);
            thread::spawn(move || {
                let msg = crate::core::cloud::pull_mobile_events_inner();
                eprintln!("{} periodic pull: {}", TAG, msg);
                if msg.starts_with("追加") || msg.contains("件処理") {
                    *crate::core::events::SESSION_CACHE.lock().unwrap() = None;
                }
                if let Ok(content) = fs::read_to_string(&ep) {
                    let push_msg = crate::core::cloud::backup_to_drive(&content);
                    eprintln!("{} periodic push: {}", TAG, push_msg);
                }
                sync_status::end(&w, None);
            });
        }

        // ── State machine ─────────────────────────────────────────────────────
        if !sleeping && idle >= threshold {
            let start_ts = ago_str(idle);
            // start_ts = last input time; user was at PC then → cancels any stale out-state
            maybe_in_house(&events_path, &start_ts);
            append_event(&events_path, &start_ts, "IDLE_START");
            eprintln!("{} IDLE_START: idle={}s", TAG, idle);
            sleeping = true;
            // Back up to Drive so Android can see the new session immediately
            let ep = events_path.clone();
            let w = weak.clone();
            sync_status::begin(&w);
            thread::spawn(move || {
                crate::core::cloud::auto_backup_after_event(&ep);
                sync_status::end(&w, None);
            });
        } else if sleeping && idle < WAKE_SECS {
            maybe_in_house(&events_path, &now_str());
            append_event(&events_path, &now_str(), "IDLE_RESUME");
            eprintln!("{} IDLE_RESUME: was sleeping {}s", TAG, idle);
            sleeping = false;
            // Driveからモバイルイベント(DEVICE_ON等)を即座にpull → キャッシュ無効化 → push
            let ep = events_path.clone();
            let w = weak.clone();
            sync_status::begin(&w);
            thread::spawn(move || {
                let msg = crate::core::cloud::pull_mobile_events_inner();
                eprintln!("{} IDLE_RESUME pull: {}", TAG, msg);
                *crate::core::events::SESSION_CACHE.lock().unwrap() = None;
                crate::core::cloud::auto_backup_after_event(&ep);
                sync_status::end(&w, None);
            });
            // Notify UI immediately so sessions panel refreshes without polling delay.
            on_session_recorded();
        }
    }
}
