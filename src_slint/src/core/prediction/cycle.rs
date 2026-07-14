// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// cycle.rs — カイ二乗ペリオドグラムによる睡眠周期の検出
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 「何時間ごとに大きく寝ているか」を、仮眠と本眠りを人力で線引きする
//        ことなく検出する（Sokolove & Bushell, 1978のカイ二乗ペリオドグラム）。
//        1回の記録内の睡眠時間が長いほど自動的に統計量への寄与が大きくなる
//        ため、短い仮眠は自然に埋もれる（大小の閾値をこちらで決め打ちしない）。
//
// 手順 : 1. 睡眠/覚醒を30分刻みのビン列に変換する（1=睡眠中, 0=覚醒中）。
//           DAY_EXCLUDEDの日は「覚醒」ではなく「欠測」として扱い、集計から
//           除外する（0で埋めると偽の周期性を作ってしまうため）。
//        2. 候補周期τ（18h〜80h）ごとに、ビン列を長さτで折り畳んで位相
//           ごとの平均を求め、位相間のばらつきと全体の分散を比較する
//           Qp統計量を計算する（自由度τ-1のカイ二乗分布に従う）。
//        3. 自由度で正規化したQp/(τ-1)が最大になるτを周期として採用する。
//
// 依存 : crate::Session, super::utils, chrono
// 公開 : `CycleResult`, `estimate_sleep_cycle`
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::Session;
use super::super::utils;
use chrono::{Duration, NaiveDateTime};

#[derive(Clone, Copy)]
pub struct CycleResult {
    pub period_hours: f64,
}

const CYCLE_BIN_MINUTES: i64 = 30;
const CYCLE_MIN_PERIOD_HOURS: f64 = 18.0;
const CYCLE_MAX_PERIOD_HOURS: f64 = 80.0;
// ペリオドグラムが安定するには、最長候補周期(80h)が何巡もするだけの記録日数が要る。
const CYCLE_MIN_TRACKED_DAYS: i64 = 14;
// 正規化振幅Qp/(τ-1)がこの値を超えない場合は「はっきりした周期が見えない」として
// 表示しない（帰無仮説下の期待値は1なので、それより十分大きいことを要求する）。
const CYCLE_MIN_SIGNIFICANCE: f64 = 2.0;

fn parse_dt(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()
}

// `sessions`は計測対象外セッションを除いたもの、`excluded_dates`はDAY_EXCLUDEDの
// 睡眠日集合（events::get_excluded_dates）を渡す想定。
pub fn estimate_sleep_cycle(
    sessions: &[Session],
    excluded_dates: &std::collections::HashSet<String>,
) -> Option<CycleResult> {
    let today_sleep_day = utils::sleep_day(chrono::Local::now().naive_local());
    // 今日はまだ記録が完結していないため、周期検出の対象からは除く（stats.rsの
    // 平均睡眠時間の考え方と同じ）。
    let period_end_day = today_sleep_day - Duration::days(1);
    let earliest_day = sessions.iter()
        .filter_map(|s| parse_dt(&s.start))
        .map(utils::sleep_day)
        .min()?;
    if (period_end_day - earliest_day).num_days() + 1 < CYCLE_MIN_TRACKED_DAYS {
        return None;
    }

    let range_start = earliest_day.and_hms_opt(utils::SLEEP_DAY_BOUNDARY_HOUR, 0, 0)?;
    let range_end = (period_end_day + Duration::days(1)).and_hms_opt(utils::SLEEP_DAY_BOUNDARY_HOUR, 0, 0)?;
    let n_bins = ((range_end - range_start).num_minutes() / CYCLE_BIN_MINUTES) as usize;
    if n_bins == 0 { return None; }

    let intervals: Vec<(NaiveDateTime, NaiveDateTime)> = sessions.iter()
        .filter_map(|s| Some((parse_dt(&s.start)?, parse_dt(&s.end)?)))
        .collect();
    let merged = utils::merge_intervals(intervals);

    // ビンごとに 睡眠中=Some(1.0) / 覚醒中=Some(0.0) / 対象外日=None を決める。
    // merged/binとも時系列順なので、ポインタmiを進めるだけの線形走査で済む。
    let mut series: Vec<Option<f64>> = Vec::with_capacity(n_bins);
    let mut mi = 0usize;
    for i in 0..n_bins {
        let bin_start = range_start + Duration::minutes(i as i64 * CYCLE_BIN_MINUTES);
        let bin_end = bin_start + Duration::minutes(CYCLE_BIN_MINUTES);
        if excluded_dates.contains(&utils::sleep_day(bin_start).format("%Y-%m-%d").to_string()) {
            series.push(None);
            continue;
        }
        while mi < merged.len() && merged[mi].1 <= bin_start { mi += 1; }
        let asleep = mi < merged.len() && merged[mi].0 < bin_end && merged[mi].1 > bin_start;
        series.push(Some(if asleep { 1.0 } else { 0.0 }));
    }

    chi_square_periodogram(&series, CYCLE_BIN_MINUTES)
}

fn chi_square_periodogram(series: &[Option<f64>], bin_minutes: i64) -> Option<CycleResult> {
    let valid: Vec<(usize, f64)> = series.iter().enumerate()
        .filter_map(|(i, v)| v.map(|x| (i, x)))
        .collect();
    if valid.len() < 20 { return None; }

    let grand_mean = valid.iter().map(|&(_, x)| x).sum::<f64>() / valid.len() as f64;
    let total_ss: f64 = valid.iter().map(|&(_, x)| (x - grand_mean).powi(2)).sum();
    // 全ビンが同じ値（ずっと寝ている/ずっと起きている）だと分散が0になり評価不能。
    if total_ss <= 0.0 { return None; }
    let variance_per_point = total_ss / (valid.len() as f64 - 1.0);

    let bins_per_hour = 60.0 / bin_minutes as f64;
    let min_p = (CYCLE_MIN_PERIOD_HOURS * bins_per_hour).round() as usize;
    let max_p = (CYCLE_MAX_PERIOD_HOURS * bins_per_hour).round() as usize;

    let mut best: Option<(usize, f64)> = None; // (周期のビン数, 正規化振幅)
    for p in min_p..=max_p {
        let mut sums = vec![0.0_f64; p];
        let mut counts = vec![0usize; p];
        for &(i, x) in &valid {
            let col = i % p;
            sums[col] += x;
            counts[col] += 1;
        }
        let mut between_ss = 0.0_f64;
        for col in 0..p {
            if counts[col] == 0 { continue; }
            let mh = sums[col] / counts[col] as f64;
            between_ss += counts[col] as f64 * (mh - grand_mean).powi(2);
        }
        let qp = between_ss / variance_per_point;
        let normalized = qp / (p as f64 - 1.0);
        if best.is_none_or(|(_, b)| normalized > b) {
            best = Some((p, normalized));
        }
    }

    best.filter(|&(_, amp)| amp >= CYCLE_MIN_SIGNIFICANCE)
        .map(|(p, _)| CycleResult { period_hours: p as f64 / bins_per_hour })
}

#[cfg(test)]
mod tests {
    use super::*;

    // period_bins周期で先頭sleep_bins個だけ1.0（睡眠中）、残りは0.0（覚醒中）を
    // 繰り返す合成データを作る。
    fn synth_series(total_bins: usize, period_bins: usize, sleep_bins: usize) -> Vec<Option<f64>> {
        (0..total_bins).map(|i| Some(if i % period_bins < sleep_bins { 1.0 } else { 0.0 })).collect()
    }

    #[test]
    fn detects_clean_48h_cycle() {
        // 30分ビンで20日分(960ビン)、48h(96ビン)ごとに8h(16ビン)だけ睡眠。
        let series = synth_series(960, 96, 16);
        let result = chi_square_periodogram(&series, 30).expect("周期が検出されるべき");
        assert!((result.period_hours - 48.0).abs() < 2.0, "period_hours={}", result.period_hours);
    }

    #[test]
    fn detects_clean_24h_cycle() {
        // 30分ビンで20日分(960ビン)、24h(48ビン)ごとに8h(16ビン)だけ睡眠。
        let series = synth_series(960, 48, 16);
        let result = chi_square_periodogram(&series, 30).expect("周期が検出されるべき");
        assert!((result.period_hours - 24.0).abs() < 2.0, "period_hours={}", result.period_hours);
    }

    #[test]
    fn flat_signal_has_no_detectable_cycle() {
        // ずっと睡眠中（分散0）だと周期性を評価できないためNoneを返す。
        let series: Vec<Option<f64>> = vec![Some(1.0); 960];
        assert!(chi_square_periodogram(&series, 30).is_none());
    }

    #[test]
    fn too_few_valid_points_returns_none() {
        let series: Vec<Option<f64>> = vec![Some(1.0), Some(0.0), None, Some(1.0)];
        assert!(chi_square_periodogram(&series, 30).is_none());
    }

    // 回帰テスト：DAY_EXCLUDEDに相当する欠測ビン(None)は「覚醒中(0.0)」として
    // 埋めてはいけない。欠測を0で埋めると、その位相が本来の周期の「睡眠中」に
    // 重なっていた場合に信号が薄まり、周期の検出精度が落ちる（最悪検出できなく
    // なる）。欠測を正しく除外すれば、一部の日が丸ごと抜けていても周期は
    // 変わらず検出できるはずであることを確認する。
    #[test]
    fn missing_bins_are_excluded_not_treated_as_awake() {
        let mut series = synth_series(960, 96, 16);
        // 5日目(index 4*48..5*48)を丸ごと欠測にする。この範囲は48hサイクルの
        // 睡眠位相と重なる日を含む。
        for i in (4 * 48)..(5 * 48) {
            series[i] = None;
        }
        let result = chi_square_periodogram(&series, 30).expect("欠測があっても周期が検出されるべき");
        assert!((result.period_hours - 48.0).abs() < 2.0, "period_hours={}", result.period_hours);
    }
}
