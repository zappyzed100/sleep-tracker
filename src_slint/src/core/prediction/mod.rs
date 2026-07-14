//! prediction/mod.rs — 睡眠時間・最適入眠時刻の機械学習予測
//!
//! 役割 : 過去の睡眠セッションを特徴量に変換し、ランダムフォレスト回帰で
//!        予測睡眠時間を計算する。セッション数が少ない場合はヒューリスティックで代替。
//!        特徴量は入眠時刻・連続起床時間・その時刻帯の過去平均・直前24h/48hの
//!        睡眠量（睡眠負債）・検出済み睡眠周期の位相・曜日。説明性より予測精度を
//!        優先し、周期(cycle.rs)の検出結果も入力に取り込んでいる。
//!
//! 依存 : crate::Session, smartcore, std::f64::consts::PI
//! 公開 : `PredictionResult`, `OptimalResult`, `predict`, `find_optimal`,
//!        `CycleResult`, `estimate_sleep_cycle`（cycle.rsに実装）

use std::f64::consts::PI;
use smartcore::ensemble::random_forest_regressor::{
    RandomForestRegressor, RandomForestRegressorParameters,
};
use smartcore::linalg::basic::matrix::DenseMatrix;

use crate::Session;

mod cycle;
pub use cycle::{CycleResult, estimate_sleep_cycle};

#[derive(serde::Serialize, Clone)]
pub struct PredictionResult {
    pub duration_hours: f64,
    pub method: String,
    pub awake_hours: f64,
}

fn bed_hour(ts: &str) -> f64 {
    let h: f64 = ts.get(11..13).unwrap_or("0").parse().unwrap_or(0.0);
    let m: f64 = ts.get(14..16).unwrap_or("0").parse().unwrap_or(0.0);
    h + m / 60.0
}

fn wake_hour(ts: &str) -> f64 {
    let h: f64 = ts.get(11..13).unwrap_or("7").parse().unwrap_or(7.0);
    let m: f64 = ts.get(14..16).unwrap_or("0").parse().unwrap_or(0.0);
    h + m / 60.0
}

// core/utils.rsのSLEEP_DAY_BOUNDARY_HOURと同じ考え方: 深夜0時ではなく午前4時を
// 1日の区切りとみなす「行動上の曜日」。1:33の睡眠開始は暦日では日曜だが、
// 体感的には土曜の夜更かしの延長なので、曜日特徴量もこの境界でシフトさせて揃える。
const SLEEP_DAY_BOUNDARY_HOUR: i64 = 4;

fn weekday_idx(ts: &str) -> usize {
    let shifted_days = (rough_epoch(ts) - SLEEP_DAY_BOUNDARY_HOUR * 3600).div_euclid(86400);
    // rough_epochの基準日(2000-01-01, shifted_days=0)は土曜日 → Mon=0基準では5。
    ((shifted_days + 5).rem_euclid(7)) as usize
}

fn rough_epoch(ts: &str) -> i64 {
    let y: i64 = ts.get(0..4).unwrap_or("2000").parse().unwrap_or(2000);
    let mo: i64 = ts.get(5..7).unwrap_or("1").parse().unwrap_or(1);
    let d: i64 = ts.get(8..10).unwrap_or("1").parse().unwrap_or(1);
    let h: i64 = ts.get(11..13).unwrap_or("0").parse().unwrap_or(0);
    let mi: i64 = ts.get(14..16).unwrap_or("0").parse().unwrap_or(0);
    let s: i64 = ts.get(17..19).unwrap_or("0").parse().unwrap_or(0);
    let base = y - 2000;
    let yd = base * 365 + base / 4 - base / 100 + base / 400;
    let md: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 1 } else { 0 };
    let days = yd + md[(mo - 1) as usize] + if mo > 2 { leap } else { 0 } + d - 1;
    days * 86400 + h * 3600 + mi * 60 + s
}

fn awake_between(end_ts: &str, start_ts: &str) -> f64 {
    ((rough_epoch(start_ts) - rough_epoch(end_ts)) as f64 / 3600.0).clamp(0.0, 48.0)
}

// Median of historical wake-up hours. Used as the default target when the user
// hasn't pinned a specific wake time in settings.
fn median_wake_hour(sessions: &[Session]) -> f64 {
    if sessions.is_empty() { return 7.0; }
    let mut hours: Vec<f64> = sessions.iter().map(|s| wake_hour(&s.end)).collect();
    hours.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = hours.len() / 2;
    if hours.len() % 2 == 0 {
        (hours[mid - 1] + hours[mid]) / 2.0
    } else {
        hours[mid]
    }
}

// Returns true if `h` (0–23.999) falls within a ±2h window around `target`.
fn in_wake_window(h: f64, target: f64) -> bool {
    let lo = (target - 2.0).rem_euclid(24.0);
    let hi = (target + 2.0).rem_euclid(24.0);
    if lo <= hi {
        h >= lo && h <= hi
    } else {
        h >= lo || h <= hi
    }
}

// Average sleep duration for sessions with a similar bedtime (±2h window).
fn hist_avg_at(sessions: &[Session], bh: f64) -> f64 {
    let similar: Vec<f64> = sessions.iter().filter_map(|s| {
        let h = bed_hour(&s.start);
        let diff = (h - bh).abs().min((h - bh + 24.0).abs()).min((h - bh - 24.0).abs());
        if diff <= 2.0 { Some(s.duration_hours) } else { None }
    }).collect();
    if !similar.is_empty() {
        similar.iter().sum::<f64>() / similar.len() as f64
    } else {
        sessions.iter().map(|s| s.duration_hours).sum::<f64>() / sessions.len().max(1) as f64
    }
}

// 1件の睡眠を予測するための入力。学習時は過去の各セッションから、予測時は
// 「これから寝る候補」から、それぞれ同じ形で組み立てる。
struct FeatureInputs {
    bed_hour: f64,      // 入眠時刻（0-23.99）
    awake_h: f64,       // 直前の睡眠からの連続起床時間
    weekday: usize,     // 行動上の曜日（Mon=0 … Sun=6）
    hist_avg: f64,      // 同じ入眠時刻帯（±2h）での過去平均睡眠時間
    past24: f64,        // 直前24hの累計睡眠時間（恒常性: 睡眠負債／寝すぎ）
    past48: f64,        // 直前48hの累計睡眠時間
    cycle_sin: f64,     // 検出した睡眠周期における位相のsin（周期未検出なら0）
    cycle_cos: f64,     // 同cos
}

// Features (17 total):
//   0-1   : sin/cos of bed hour (24h cycle)
//   2     : hours awake before this sleep
//   3     : historical avg duration at this bedtime
//   4     : total sleep in the past 24h (homeostatic pressure / oversleep)
//   5     : total sleep in the past 48h
//   6-7   : sin/cos of phase within the detected multi-day sleep cycle
//   8     : whether a cycle was detected (1) or not (0)
//   9-15  : weekday one-hot (Mon=0 … Sun=6)
fn make_features(f: &FeatureInputs) -> Vec<f64> {
    let has_cycle = if f.cycle_sin != 0.0 || f.cycle_cos != 0.0 { 1.0 } else { 0.0 };
    let mut v = vec![
        (f.bed_hour * 2.0 * PI / 24.0).sin(),
        (f.bed_hour * 2.0 * PI / 24.0).cos(),
        f.awake_h,
        f.hist_avg,
        f.past24,
        f.past48,
        f.cycle_sin,
        f.cycle_cos,
        has_cycle,
    ];
    for i in 0..7 {
        v.push(if i == f.weekday { 1.0 } else { 0.0 });
    }
    v
}

// 指定時刻(before_epoch)より前、window_secs以内に始まった睡眠の合計時間。
// 睡眠負債（直前にどれだけ寝ているか）を表す特徴量。
fn past_sleep_hours(sessions: &[Session], before_epoch: i64, window_secs: i64) -> f64 {
    sessions.iter().filter_map(|s| {
        let st = rough_epoch(&s.start);
        if st < before_epoch && st >= before_epoch - window_secs {
            Some(s.duration_hours)
        } else {
            None
        }
    }).sum()
}

// 検出済みの睡眠周期(period_hours)における、bed_epochの位相をsin/cosで表す。
// anchorは最初の記録の開始時刻を0とする基準。周期が未検出なら(0,0)を返し、
// make_features側で「周期なし」フラグに変換される。
fn cycle_sincos(bed_epoch: i64, anchor: i64, period_hours: Option<f64>) -> (f64, f64) {
    match period_hours {
        Some(p) if p > 0.0 => {
            let hours = (bed_epoch - anchor) as f64 / 3600.0;
            let frac = (hours / p).rem_euclid(1.0);
            ((frac * 2.0 * PI).sin(), (frac * 2.0 * PI).cos())
        }
        _ => (0.0, 0.0),
    }
}

// 学習用: 各セッションから特徴量ベクトルを組み立てる（predict/find_optimal共通）。
fn build_training_rows(sessions: &[Session], anchor: i64, cycle_period: Option<f64>) -> Vec<Vec<f64>> {
    sessions.iter().enumerate().map(|(i, s)| {
        let aw = if i == 0 { 16.0 } else { awake_between(&sessions[i - 1].end, &s.start) };
        let sbh = bed_hour(&s.start);
        let st = rough_epoch(&s.start);
        let (cs, cc) = cycle_sincos(st, anchor, cycle_period);
        make_features(&FeatureInputs {
            bed_hour: sbh,
            awake_h: aw,
            weekday: weekday_idx(&s.start),
            hist_avg: hist_avg_at(sessions, sbh),
            past24: past_sleep_hours(sessions, st, 24 * 3600),
            past48: past_sleep_hours(sessions, st, 48 * 3600),
            cycle_sin: cs,
            cycle_cos: cc,
        })
    }).collect()
}

fn heuristic(sessions: &[Session], bh: f64) -> (f64, String) {
    let similar: Vec<f64> = sessions
        .iter()
        .filter_map(|s| {
            let h = bed_hour(&s.start);
            let diff = (h - bh).abs().min((h - bh + 24.0).abs()).min((h - bh - 24.0).abs());
            if diff <= 2.0 { Some(s.duration_hours) } else { None }
        })
        .collect();

    if similar.len() >= 3 {
        let avg = similar.iter().sum::<f64>() / similar.len() as f64;
        return (avg, format!("類似{}件", similar.len()));
    }

    let mut all: Vec<f64> = sessions.iter().map(|s| s.duration_hours).collect();
    if all.is_empty() {
        return (7.5, "初期値".to_string());
    }
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo = all.len() / 10;
    let hi = (all.len() - all.len() / 10).max(lo + 1);
    let avg = all[lo..hi].iter().sum::<f64>() / (hi - lo) as f64;
    (avg, "全体平均".to_string())
}

#[derive(serde::Serialize, Clone)]
pub struct OptimalResult {
    pub best_bed_time: String,
    pub expected_wake_time: String,
    pub duration_hours: f64,
}

// `target_wake_hhmm`: user-pinned wake time ("HH:MM"), or None → use median of history.
// Strategy: among the 24 bedtime candidates (every 30 min, next 12h), keep only those
// whose predicted wake-up time falls within ±2h of the target, then pick the one with
// the shortest predicted sleep duration.  This avoids selecting nap-right-after-waking
// scenarios and encodes "which bedtime lets you wake at the right time most efficiently."
// Falls back to global min-duration if no candidate fits the window.
pub fn find_optimal(sessions: &[Session], now: &str, target_wake_hhmm: Option<&str>, cycle_period: Option<f64>) -> Option<OptimalResult> {
    if sessions.is_empty() {
        return None;
    }

    let now_h: i64 = now.get(11..13).unwrap_or("0").parse().unwrap_or(0);
    let now_m: i64 = now.get(14..16).unwrap_or("0").parse().unwrap_or(0);
    let wd_base = weekday_idx(now);
    let awake_h = sessions.last().map(|s| awake_between(&s.end, now)).unwrap_or(16.0);
    let now_epoch = rough_epoch(now);
    let anchor = sessions.iter().map(|s| rough_epoch(&s.start)).min().unwrap_or(now_epoch);

    // Determine target wake hour
    let target_wake: f64 = if let Some(hhmm) = target_wake_hhmm {
        let h: f64 = hhmm.get(0..2).unwrap_or("7").parse().unwrap_or(7.0);
        let m: f64 = hhmm.get(3..5).unwrap_or("0").parse().unwrap_or(0.0);
        h + m / 60.0
    } else {
        median_wake_hour(sessions)
    };

    // 24 candidates: every 30 min from now+30 to now+12h
    struct Candidate { bh: f64, wd: usize, h: i64, m: i64, slot_mins: i64 }
    let candidates: Vec<Candidate> = (1i64..=24)
        .map(|slot| {
            let total = now_h * 60 + now_m + slot * 30;
            let h = (total / 60) % 24;
            let m = total % 60;
            let bh = h as f64 + m as f64 / 60.0;
            let wd = (wd_base + (total / (24 * 60)) as usize) % 7;
            Candidate { bh, wd, h, m, slot_mins: slot * 30 }
        })
        .collect();

    let durations: Vec<f64> = if sessions.len() >= 10 {
        let x_rows = build_training_rows(sessions, anchor, cycle_period);
        let y: Vec<f64> = sessions.iter().map(|s| s.duration_hours).collect();
        let feat_list: Vec<Vec<f64>> = candidates
            .iter()
            .map(|c| {
                let cand_epoch = now_epoch + c.slot_mins * 60;
                let (cs, cc) = cycle_sincos(cand_epoch, anchor, cycle_period);
                make_features(&FeatureInputs {
                    bed_hour: c.bh,
                    awake_h,
                    weekday: c.wd,
                    hist_avg: hist_avg_at(sessions, c.bh),
                    past24: past_sleep_hours(sessions, cand_epoch, 24 * 3600),
                    past48: past_sleep_hours(sessions, cand_epoch, 48 * 3600),
                    cycle_sin: cs,
                    cycle_cos: cc,
                })
            })
            .collect();
        let x_mat = DenseMatrix::from_2d_vec(&x_rows);
        let x_q   = DenseMatrix::from_2d_vec(&feat_list);
        let params = RandomForestRegressorParameters { n_trees: 50, ..Default::default() };
        match RandomForestRegressor::fit(&x_mat, &y, params).and_then(|m| m.predict(&x_q)) {
            Ok(preds) => preds.into_iter().map(|d| d.clamp(1.0, 18.0)).collect(),
            Err(_)    => candidates.iter().map(|c| heuristic(sessions, c.bh).0).collect(),
        }
    } else {
        candidates.iter().map(|c| heuristic(sessions, c.bh).0).collect()
    };

    // For each candidate, compute predicted wake hour of day
    let wake_hours: Vec<f64> = candidates.iter().zip(durations.iter()).map(|(c, &dur)| {
        let wake_total = now_h * 60 + now_m + c.slot_mins + (dur * 60.0) as i64;
        (wake_total / 60) as f64 % 24.0
    }).collect();

    // Among candidates whose wake hour is in the ±2h target window, pick minimum duration.
    // Fall back to global min-duration if the window yields nothing.
    let best_idx = {
        let in_window: Vec<usize> = (0..candidates.len())
            .filter(|&i| in_wake_window(wake_hours[i], target_wake))
            .collect();
        if !in_window.is_empty() {
            in_window.into_iter()
                .min_by(|&a, &b| durations[a].partial_cmp(&durations[b]).unwrap_or(std::cmp::Ordering::Equal))
        } else {
            (0..candidates.len())
                .min_by(|&a, &b| durations[a].partial_cmp(&durations[b]).unwrap_or(std::cmp::Ordering::Equal))
        }
    };

    best_idx.map(|i| {
        let c = &candidates[i];
        let dur = durations[i];
        let wake_total = now_h * 60 + now_m + c.slot_mins + (dur * 60.0) as i64;
        let wake_h = (wake_total / 60) % 24;
        let wake_m = wake_total % 60;
        OptimalResult {
            best_bed_time:      format!("{:02}:{:02}", c.h, c.m),
            expected_wake_time: format!("{:02}:{:02}", wake_h, wake_m),
            duration_hours:     dur,
        }
    })
}

pub fn predict(sessions: &[Session], now: &str, cycle_period: Option<f64>) -> PredictionResult {
    let bh      = bed_hour(now);
    let wd      = weekday_idx(now);
    let awake_h = sessions.last().map(|s| awake_between(&s.end, now)).unwrap_or(16.0);

    if sessions.len() < 10 {
        let (dur, method) = heuristic(sessions, bh);
        return PredictionResult { duration_hours: dur.clamp(1.0, 18.0), method, awake_hours: awake_h };
    }

    let now_epoch = rough_epoch(now);
    let anchor = sessions.iter().map(|s| rough_epoch(&s.start)).min().unwrap_or(now_epoch);
    let x_rows = build_training_rows(sessions, anchor, cycle_period);
    let y: Vec<f64> = sessions.iter().map(|s| s.duration_hours).collect();

    let (cs, cc) = cycle_sincos(now_epoch, anchor, cycle_period);
    let q = make_features(&FeatureInputs {
        bed_hour: bh,
        awake_h,
        weekday: wd,
        hist_avg: hist_avg_at(sessions, bh),
        past24: past_sleep_hours(sessions, now_epoch, 24 * 3600),
        past48: past_sleep_hours(sessions, now_epoch, 48 * 3600),
        cycle_sin: cs,
        cycle_cos: cc,
    });

    let x_mat = DenseMatrix::from_2d_vec(&x_rows);
    let x_q   = DenseMatrix::from_2d_vec(&vec![q]);
    let params = RandomForestRegressorParameters { n_trees: 50, ..Default::default() };

    match RandomForestRegressor::fit(&x_mat, &y, params) {
        Ok(model) => match model.predict(&x_q) {
            Ok(preds) => PredictionResult {
                duration_hours: preds[0].clamp(1.0, 18.0),
                method: "機械学習".to_string(),
                awake_hours: awake_h,
            },
            Err(_) => {
                let (dur, method) = heuristic(sessions, bh);
                PredictionResult { duration_hours: dur.clamp(1.0, 18.0), method, awake_hours: awake_h }
            }
        },
        Err(_) => {
            let (dur, method) = heuristic(sessions, bh);
            PredictionResult { duration_hours: dur.clamp(1.0, 18.0), method, awake_hours: awake_h }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2024-01-07は日曜日。1:33開始の睡眠は、境界(4時)より前なので
    // 「行動上の曜日」としては土曜(index 5)として扱われるべき
    // （core/utils.rsのsleep_dayと同じ境界・同じ考え方）。
    #[test]
    fn weekday_idx_before_boundary_counts_as_previous_day() {
        assert_eq!(weekday_idx("2024-01-07 01:33:00"), 5); // 土曜
        assert_eq!(weekday_idx("2024-01-07 15:46:00"), 6); // 日曜（境界後はそのまま）
    }

    #[test]
    fn weekday_idx_boundary_is_inclusive_at_exactly_4am() {
        assert_eq!(weekday_idx("2024-01-07 04:00:00"), 6); // 日曜のまま
        assert_eq!(weekday_idx("2024-01-07 03:59:59"), 5); // 土曜に繰り下げ
    }

    #[test]
    fn weekday_idx_evening_hour_is_unaffected_by_boundary() {
        assert_eq!(weekday_idx("2024-01-06 23:50:00"), 5); // 土曜
    }
}
