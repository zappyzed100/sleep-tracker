//! calendar.rs — カレンダーピッカー（CalendarPicker.tsx 相当）
//!
//! 役割 : 月送り・日付タップによる週選択のカレンダーモーダルを担当する。
//!
//! 依存 : super::{state::SharedState, chart::update_chart}, crate::core::utils,
//!        crate::{CalendarDayVM, MainWindow}

use super::chart::update_chart;
use super::state::SharedState;
use crate::core::utils;
use crate::{CalendarDayVM, MainWindow};
use chrono::{Datelike, NaiveDate};

fn shift_month(d: NaiveDate, delta: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month() as i32 - 1 + delta;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    NaiveDate::from_ymd_opt(y, m as u32, 1).unwrap()
}

fn build_calendar_days(view: NaiveDate, week_base: NaiveDate) -> Vec<CalendarDayVM> {
    use chrono::Local;
    let today = Local::now().date_naive();
    let first_of_month = NaiveDate::from_ymd_opt(view.year(), view.month(), 1).unwrap();
    let grid_start = utils::week_start(first_of_month);
    let ws = utils::week_start(week_base);
    let we = ws + chrono::Duration::days(6);

    (0..42).map(|i| {
        let d = grid_start + chrono::Duration::days(i);
        CalendarDayVM {
            day: d.day() as i32,
            date: d.format("%Y-%m-%d").to_string().into(),
            in_month: d.month() == view.month(),
            is_today: d == today,
            in_week: d >= ws && d <= we,
        }
    }).collect()
}

fn refresh_calendar(window: &MainWindow, state: &SharedState) {
    let (view, week_base) = {
        let s = state.lock().unwrap();
        (s.cal_view, s.week_base)
    };
    window.set_cal_month_label(format!("{}年{}月", view.year(), view.month()).into());
    window.set_cal_days(slint::ModelRc::new(slint::VecModel::from(build_calendar_days(view, week_base))));
}

pub fn open_calendar(window: &MainWindow, state: &SharedState) {
    {
        let week_base = state.lock().unwrap().week_base;
        state.lock().unwrap().cal_view = NaiveDate::from_ymd_opt(week_base.year(), week_base.month(), 1).unwrap();
    }
    refresh_calendar(window, state);
    window.set_cal_open(true);
}

pub fn close_calendar(window: &MainWindow) {
    window.set_cal_open(false);
}

pub fn cal_prev_month(window: &MainWindow, state: &SharedState) {
    { let mut s = state.lock().unwrap(); s.cal_view = shift_month(s.cal_view, -1); }
    refresh_calendar(window, state);
}

pub fn cal_next_month(window: &MainWindow, state: &SharedState) {
    { let mut s = state.lock().unwrap(); s.cal_view = shift_month(s.cal_view, 1); }
    refresh_calendar(window, state);
}

pub fn cal_select_day(window: &MainWindow, state: &SharedState, date: &str) {
    if let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        state.lock().unwrap().week_base = d;
    }
    window.set_cal_open(false);
    update_chart(window, state);
}
