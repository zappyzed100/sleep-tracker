//! state.rs — アプリ共有状態（AppState/SharedState）と期間タブ・週ナビゲーション
//!
//! 役割 : バックグラウンドスレッド（Drive同期・監視スレッド）から
//!        slint::invoke_from_event_loop 経由でUIスレッドに戻すため Arc<Mutex<>> で
//!        保持する共有状態(AppState)の定義、期間タブ(Period)、週送り・全体再読み込み
//!        (refresh_all)を担当する。
//!
//! 依存 : super::{stats, chart}, crate::{MainWindow}

use crate::MainWindow;
use chrono::{Datelike, NaiveDate};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// StatsCard.tsx の期間タブ（先週=7日・先月=30日・1年=365日・全期間）相当。
#[derive(Clone, Copy, PartialEq)]
pub enum Period {
    Week,
    Month,
    Year,
    All,
}

impl Period {
    fn from_key(key: &str) -> Self {
        match key {
            "week" => Period::Week,
            "year" => Period::Year,
            "all" => Period::All,
            _ => Period::Month,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Period::Week => "week",
            Period::Month => "month",
            Period::Year => "year",
            Period::All => "all",
        }
    }

    pub(super) fn days(self) -> Option<i64> {
        match self {
            Period::Week => Some(7),
            Period::Month => Some(30),
            Period::Year => Some(365),
            Period::All => None,
        }
    }
}

pub(super) fn awake_color(h: f64) -> slint::Color {
    if h > 16.0 { slint::Color::from_rgb_u8(0xf3, 0x8b, 0xa8) }      // --red
    else if h > 12.0 { slint::Color::from_rgb_u8(0xf9, 0xe2, 0xaf) } // --yellow
    else { slint::Color::from_rgb_u8(0xa6, 0xe3, 0xa1) }             // --green
}

pub fn now_iso() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub(super) fn bed_time_to_iso(h: i32, m: i32) -> String {
    use chrono::Local;
    format!("{} {:02}:{:02}:00", Local::now().format("%Y-%m-%d"), h, m)
}

// ── アプリ共有状態 ────────────────────────────────────────────────────────────
//
// バックグラウンドスレッド（Drive同期・監視スレッド）から
// slint::invoke_from_event_loop 経由でUIスレッドに戻すため Arc<Mutex<>> で保持する
// （Rc<RefCell<>> は Send でないため thread::spawn の境界を越えられない）。

pub(super) struct StatsBaseline {
    pub(super) awake_hours: f64,
    pub(super) computed_at: Instant,
}

pub struct AppState {
    pub(super) baseline: Option<StatsBaseline>,
    // 進行中（まだIDLE_RESUMEが来ていない）睡眠セッションの開始時刻。
    // 暫定睡眠時間の表示用（寝ている最中に一瞬起きて確認する用途）。
    // 絶対時刻で持っているので、tickごとに Local::now() との差分を取り直すだけでよい
    // （awake_hoursのようにInstant経過分を毎回加算する必要がない）。
    pub(super) open_sleep_start: Option<chrono::NaiveDateTime>,
    // open_sleep_startが属する睡眠日の、確定済み（閉じた）セッションだけの合計時間。
    // 「最後の睡眠」表示はこれにapply_tickで進行中セッションの経過時間を足し合わせる
    // （compute_stats参照）。
    pub(super) last_day_confirmed_hours: Option<f64>,
    pub(super) week_base: NaiveDate,
    pub(super) selected_date: Option<String>,
    pub(super) period: Period,
    pub(super) cal_view: NaiveDate,
}

pub type SharedState = Arc<Mutex<AppState>>;

pub fn new_shared_state() -> SharedState {
    use chrono::Local;
    let today = Local::now().date_naive();
    Arc::new(Mutex::new(AppState {
        baseline: None,
        open_sleep_start: None,
        last_day_confirmed_hours: None,
        week_base: today,
        selected_date: None,
        period: Period::Month,
        cal_view: NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    }))
}

// ── 統計ストリップ（StatsCard.tsx 相当）────────────────────────────────────────

pub fn set_period(window: &MainWindow, state: &SharedState, key: &str) {
    let period = Period::from_key(key);
    state.lock().unwrap().period = period;
    window.set_period_key(period.key().into());
    super::stats::compute_stats(window, state);
}

pub fn refresh_all(window: &MainWindow, state: &SharedState) {
    super::stats::compute_stats(window, state);
    super::stats::recompute_prediction(window);
    super::chart::update_chart(window, state);
}

pub fn adjust_week(state: &SharedState, days: i64) {
    state.lock().unwrap().week_base += chrono::Duration::days(days);
}

pub fn reset_week_to_today(state: &SharedState) {
    use chrono::Local;
    state.lock().unwrap().week_base = Local::now().date_naive();
}

pub fn selected_date(state: &SharedState) -> Option<String> {
    state.lock().unwrap().selected_date.clone()
}
