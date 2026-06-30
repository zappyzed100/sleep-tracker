//! prediction.rs — 睡眠時間・最適入眠時刻の機械学習予測
//!
//! 役割 : 過去の睡眠セッションを特徴量に変換し、ランダムフォレスト回帰で
//!        予測睡眠時間を計算する。セッション数が少ない場合はヒューリスティックで代替。
//!
//! 依存 : crate::Session, smartcore, std::f64::consts::PI
//! 公開 : `PredictionResult`, `OptimalResult`, `predict`, `find_optimal`

use std::f64::consts::PI;
use smartcore::ensemble::random_forest_regressor::{
    RandomForestRegressor, RandomForestRegressorParameters,
};
use smartcore::linalg::basic::matrix::DenseMatrix;

use crate::Session;

#[derive(serde::Serialize)]
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

fn weekday_idx(ts: &str) -> usize {
    let y: i64 = ts.get(0..4).unwrap_or("2000").parse().unwrap_or(2000);
    let m: i64 = ts.get(5..7).unwrap_or("1").parse().unwrap_or(1);
    let d: i64 = ts.get(8..10).unwrap_or("1").parse().unwrap_or(1);
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let k = y % 100;
    let j = y / 100;
    let h = (d + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 - 2 * j).rem_euclid(7) as usize;
    (h + 5) % 7
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

// Features (11 total):
//   0-1  : sin/cos of bed hour (24h cycle)
//   2    : hours awake before this sleep
//   3    : historical avg duration at this bedtime (explicit bedtime→duration signal)
//   4-10 : weekday one-hot (Mon=0 … Sun=6)
fn make_features(bh: f64, awake_h: f64, wd: usize, hist_avg: f64) -> Vec<f64> {
    let mut f = vec![
        (bh * 2.0 * PI / 24.0).sin(),
        (bh * 2.0 * PI / 24.0).cos(),
        awake_h,
        hist_avg,
    ];
    for i in 0..7 {
        f.push(if i == wd { 1.0 } else { 0.0 });
    }
    f
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
        return (avg, format!("Heuristic ({} similar sessions)", similar.len()));
    }

    let mut all: Vec<f64> = sessions.iter().map(|s| s.duration_hours).collect();
    if all.is_empty() {
        return (7.5, "Default".to_string());
    }
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo = all.len() / 10;
    let hi = (all.len() - all.len() / 10).max(lo + 1);
    let avg = all[lo..hi].iter().sum::<f64>() / (hi - lo) as f64;
    (avg, "Heuristic (global avg)".to_string())
}

#[derive(serde::Serialize)]
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
pub fn find_optimal(sessions: &[Session], now: &str, target_wake_hhmm: Option<&str>) -> Option<OptimalResult> {
    if sessions.is_empty() {
        return None;
    }

    let now_h: i64 = now.get(11..13).unwrap_or("0").parse().unwrap_or(0);
    let now_m: i64 = now.get(14..16).unwrap_or("0").parse().unwrap_or(0);
    let wd_base = weekday_idx(now);
    let awake_h = sessions.last().map(|s| awake_between(&s.end, now)).unwrap_or(16.0);

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
        let x_rows: Vec<Vec<f64>> = sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let aw = if i == 0 { 16.0 } else { awake_between(&sessions[i - 1].end, &s.start) };
                let bh = bed_hour(&s.start);
                make_features(bh, aw, weekday_idx(&s.start), hist_avg_at(sessions, bh))
            })
            .collect();
        let y: Vec<f64> = sessions.iter().map(|s| s.duration_hours).collect();
        let feat_list: Vec<Vec<f64>> = candidates
            .iter()
            .map(|c| make_features(c.bh, awake_h, c.wd, hist_avg_at(sessions, c.bh)))
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

pub fn predict(sessions: &[Session], now: &str) -> PredictionResult {
    let bh      = bed_hour(now);
    let wd      = weekday_idx(now);
    let awake_h = sessions.last().map(|s| awake_between(&s.end, now)).unwrap_or(16.0);

    if sessions.len() < 10 {
        let (dur, method) = heuristic(sessions, bh);
        return PredictionResult { duration_hours: dur.clamp(1.0, 18.0), method, awake_hours: awake_h };
    }

    let hist = hist_avg_at(sessions, bh);
    let x_rows: Vec<Vec<f64>> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let aw  = if i == 0 { 16.0 } else { awake_between(&sessions[i - 1].end, &s.start) };
            let sbh = bed_hour(&s.start);
            make_features(sbh, aw, weekday_idx(&s.start), hist_avg_at(sessions, sbh))
        })
        .collect();
    let y: Vec<f64> = sessions.iter().map(|s| s.duration_hours).collect();

    let x_mat = DenseMatrix::from_2d_vec(&x_rows);
    let x_q   = DenseMatrix::from_2d_vec(&vec![make_features(bh, awake_h, wd, hist)]);
    let params = RandomForestRegressorParameters { n_trees: 50, ..Default::default() };

    match RandomForestRegressor::fit(&x_mat, &y, params) {
        Ok(model) => match model.predict(&x_q) {
            Ok(preds) => PredictionResult {
                duration_hours: preds[0].clamp(1.0, 18.0),
                method: format!("Machine Learning (awake {:.1}h, hist {:.1}h)", awake_h, hist),
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
