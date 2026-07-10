//! home.rs — ホーム画面（統計・睡眠予測・週間チャート・日別詳細）のUI連携（エントリポイント）
//!
//! 役割 : StatsCard/PredictionCard/WeeklyChart/DayDetail各TSXコンポーネント相当の
//!        表示更新ロジックを実装する各サブモジュール（state/stats/chart/calendar/day_detail）
//!        の公開関数を束ねて再公開する。main.rsのmain()から呼ばれるコールバック本体は
//!        ここ経由でのみ使う。main.rsは配線のみを担当する。
//!
//! 依存 : crate::{MainWindow}, crate::core::{events, prediction, utils}, ui::settings_ui
//! 公開 : `AppState`, `SharedState`, `new_shared_state`, `refresh_all`,
//!        `compute_stats`, `apply_tick`, `recompute_prediction`, `update_chart`,
//!        `open_day_detail`, `close_day_detail`, `toggle_day_excluded`, `now_iso`, `set_period`,
//!        `adjust_week`, `reset_week_to_today`, `selected_date`,
//!        `open_calendar`, `close_calendar`, `cal_prev_month`, `cal_next_month`, `cal_select_day`

mod state;
mod stats;
mod chart;
mod calendar;
mod day_detail;

pub use state::{
    AppState, SharedState, new_shared_state, now_iso, set_period, refresh_all,
    adjust_week, reset_week_to_today, selected_date,
};
pub use stats::{compute_stats, apply_tick, recompute_prediction};
pub use chart::update_chart;
pub use calendar::{open_calendar, close_calendar, cal_prev_month, cal_next_month, cal_select_day};
pub use day_detail::{open_day_detail, close_day_detail, toggle_day_excluded};
