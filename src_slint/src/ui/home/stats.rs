//! stats.rs — 統計ストリップ・睡眠予測カードの計算とtick更新
//!
//! 役割 : StatsCard.tsx/PredictionCard.tsx相当の表示更新。日ごとの睡眠時間集計・
//!        「最後の睡眠」（本眠り基準の起床時刻含む）・起きてからの経過時間の
//!        軽量tick更新・睡眠予測の再計算を担当する。
//!
//! 依存 : super::{state::*, chart::update_chart}, crate::core::{events, prediction, utils, Session}

use super::chart::update_chart;
use super::state::{awake_color, bed_time_to_iso, now_iso, SharedState, StatsBaseline};
use crate::core::{events, prediction, utils, Session};
use crate::MainWindow;
use chrono::NaiveDate;
use std::time::Instant;

pub fn compute_stats(window: &MainWindow, state: &SharedState) {
    let sessions = events::get_sessions().unwrap_or_default();
    let period = state.lock().unwrap().period;

    let now = now_iso();
    let recent: Vec<&Session> = match period.days() {
        Some(days) => {
            use chrono::{Duration, Local};
            let cutoff_ts = (Local::now() - Duration::days(days)).format("%Y-%m-%d %H:%M:%S").to_string();
            sessions.iter().filter(|s| s.start.as_str() >= cutoff_ts.as_str()).collect()
        }
        None => sessions.iter().collect(),
    };

    // 平均睡眠は「セッション数」ではなく「記録日数」で割る。1日に複数セッション
    // （昼寝＋本睡眠など）があると、セッション単位の平均では1日あたりの睡眠時間が
    // 薄まって過小評価されてしまうため、日ごとに睡眠時間を合算してから平均する。
    // PC/Android両方から記録された重複区間は merge_intervals で1本にまとめてから
    // 合算する（重なった分の二重計上を防ぐ）。
    // 日ごとの集計キーは暦日ではなく「睡眠日」（開始時刻から導出、境界は午前4時）を使う。
    // 深夜1:33開始の睡眠が体感通り前日側に計上されるようにするため。
    let mut per_day_intervals: std::collections::HashMap<NaiveDate, Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)>> = std::collections::HashMap::new();
    for s in recent.iter().filter(|s| !s.excluded) {
        if let (Ok(st), Ok(en)) = (
            chrono::NaiveDateTime::parse_from_str(s.start.trim(), "%Y-%m-%d %H:%M:%S"),
            chrono::NaiveDateTime::parse_from_str(s.end.trim(), "%Y-%m-%d %H:%M:%S"),
        ) {
            per_day_intervals.entry(utils::sleep_day(st)).or_default().push((st, en));
        }
    }
    let per_day: std::collections::HashMap<NaiveDate, f64> = per_day_intervals.into_iter()
        .map(|(day, intervals)| {
            let merged = utils::merge_intervals(intervals);
            let hours: f64 = merged.iter().map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0).sum();
            (day, hours)
        })
        .collect();
    let avg = if !per_day.is_empty() {
        Some(per_day.values().sum::<f64>() / per_day.len() as f64)
    } else {
        None
    };
    // 進行中（まだ閉じていない）睡眠セッションがあれば、暫定睡眠時間表示のために
    // 開始時刻を保持する。寝ている最中に一瞬起きてタブレットを確認する用途。
    let open_sleep_start = events::current_sleep_start()
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok());

    // 「最後の睡眠」は最後のセッション1件の duration_hours ではなく、その睡眠日に
    // 計上される確定済み（閉じた）セッションを合算した値にする。ある1回の連続した
    // 睡眠が（短い中断などで）複数セッションに分かれて記録されていた場合でも、
    // 体感通りの合計時間（例: 16h）を表示するため。
    // 進行中セッションがある場合は「その睡眠日」を基準にする（1回目の睡眠が
    // 確定した後、2回目の睡眠が始まった状態でPCのIDLE_RESUME前にAndroidを確認
    // したときも、確定済み分だけの古い値ではなく、進行中セッションの経過時間を
    // 加えたライブの合計をapply_tickで表示するため。確定分はここではまだ加算
    // せず保持だけしておき、実際の加算はapply_tickが毎tick行う）。
    let last_day = open_sleep_start.map(utils::sleep_day).or_else(|| {
        sessions.last().and_then(|s| chrono::NaiveDateTime::parse_from_str(s.start.trim(), "%Y-%m-%d %H:%M:%S").ok())
            .map(utils::sleep_day)
    });
    let last_day_merged = last_day.map(|day| {
        let intervals: Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)> = sessions.iter()
            .filter_map(|s| {
                let st = chrono::NaiveDateTime::parse_from_str(s.start.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
                if utils::sleep_day(st) != day { return None; }
                let en = chrono::NaiveDateTime::parse_from_str(s.end.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
                Some((st, en))
            })
            .collect();
        utils::merge_intervals(intervals)
    });
    let last_day_confirmed_hours = last_day_merged.as_ref().map(|merged| {
        merged.iter().map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0).sum::<f64>()
    });
    // 「起床時刻」はその睡眠日の最も長いセッション（本眠り）の終了時刻を採用する。
    // 単純に最後のセッションを使うと、本眠りの後に短い仮眠を記録した場合に
    // 仮眠の終了時刻が表示されてしまう（utils::single_day_summaryのチャート表示と同じ考え方）。
    let wake_time = last_day_merged.as_ref()
        .and_then(|merged| merged.iter().max_by(|a, b| (a.1 - a.0).cmp(&(b.1 - b.0))))
        .map(|(_, e)| e.format("%H:%M").to_string());

    window.set_days_recorded(format!("{}日", per_day.len()).into());
    window.set_avg_sleep(avg.map(utils::format_duration).unwrap_or_else(|| "—".into()).into());
    window.set_wake_time(wake_time.unwrap_or_else(|| "—".to_string()).into());

    // 予測計算も計測対象外の日を除外したセッションだけで行う。
    let for_prediction: Vec<Session> = sessions.iter().filter(|s| !s.excluded).cloned().collect();
    let pred = prediction::predict(&for_prediction, &now);

    {
        let mut st = state.lock().unwrap();
        st.baseline = Some(StatsBaseline { awake_hours: pred.awake_hours, computed_at: Instant::now() });
        st.open_sleep_start = open_sleep_start;
        st.last_day_confirmed_hours = last_day_confirmed_hours;
    }

    apply_tick(window, state);
}

// 現在時刻・起きてから経過時間だけを軽量に更新する（10秒ごと）。
// 進行中の睡眠セッションがある間は「起きてから」は意味を持たない（既に
// 寝ているのに古い起床時刻からの経過時間を表示し続けるとおかしく見える、
// というフィードバックへの対応）ため、その場合は表示しない。
// 進行中セッションの経過時間そのものはグラフ側のバーで表示する
// （update_chartが担当、apply_tickからも再計算のため呼び出す）。
pub fn apply_tick(window: &MainWindow, state: &SharedState) {
    window.set_current_time(now_hhmm().into());
    let st = state.lock().unwrap();
    let is_sleeping = st.open_sleep_start.is_some();
    if is_sleeping {
        window.set_awake_since("—".into());
        window.set_awake_color(slint::Color::from_rgb_u8(0xa6, 0xad, 0xc8));
    } else if let Some(b) = st.baseline.as_ref() {
        let elapsed_h = b.computed_at.elapsed().as_secs_f64() / 3600.0;
        let awake = b.awake_hours + elapsed_h;
        window.set_awake_since(utils::format_duration(awake).into());
        window.set_awake_color(awake_color(awake));
    }
    // 「最後の睡眠」= その睡眠日の確定済み合計 + 進行中セッションの経過時間（あれば）。
    // 2回目の睡眠が始まった直後、PCのIDLE_RESUME前にAndroidを確認したような場合でも、
    // 確定済み分だけの古い値ではなく暫定分を足したライブの合計を表示する
    // （compute_stats参照）。
    let last_total = st.last_day_confirmed_hours.map(|confirmed| {
        let live_h = st.open_sleep_start
            .map(|start| (chrono::Local::now().naive_local() - start).num_seconds() as f64 / 3600.0)
            .unwrap_or(0.0);
        confirmed + live_h.max(0.0)
    });
    drop(st);
    window.set_last_sleep(last_total.map(utils::format_duration).unwrap_or_else(|| "—".into()).into());

    update_chart(window, state);
}

// ── 睡眠予測カード（PredictionCard.tsx 相当）───────────────────────────────────

fn now_hhmm() -> String {
    use chrono::Local;
    Local::now().format("%H:%M").to_string()
}

pub fn recompute_prediction(window: &MainWindow) {
    let h = window.get_bed_hour();
    let m = window.get_bed_minute();
    window.set_bed_time_label(format!("{:02}:{:02}", h, m).into());

    let sessions: Vec<Session> = events::get_sessions().unwrap_or_default()
        .into_iter().filter(|s| !s.excluded).collect();
    if sessions.is_empty() {
        window.set_has_prediction(false);
        return;
    }
    let now_at_bedtime = bed_time_to_iso(h, m);
    let pred = prediction::predict(&sessions, &now_at_bedtime);

    let wake_total_min = h * 60 + m + (pred.duration_hours * 60.0) as i32;
    let wake_h = (wake_total_min / 60).rem_euclid(24);
    let wake_m = wake_total_min.rem_euclid(60);

    window.set_predicted_duration(utils::format_duration(pred.duration_hours).into());
    window.set_predicted_wake_time(format!("{:02}:{:02}", wake_h, wake_m).into());
    window.set_predicted_method(pred.method.into());
    window.set_has_prediction(true);
}
