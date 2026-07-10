//! chart.rs — 週間チャート（WeeklyChart.tsx 相当）
//!
//! 役割 : 週データの取得・軸ラベル生成・Catmull-Romスプラインによる就寝/起床
//!        折れ線の曲線化・週境界クリップを担当する。
//!
//! 依存 : super::state::SharedState, crate::core::{config, events, utils},
//!        crate::{CurvePointVM, DaySummaryVM, MainWindow}

use super::state::SharedState;
use crate::core::{config, events, utils};
use crate::{CurvePointVM, DaySummaryVM, MainWindow};
use chrono::{Datelike, NaiveDate};

const DAYS_JA: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

// WeeklyChart.tsx の durations/y軸目盛り相当。左軸は0h〜(最大値+1)h、7段階で均等割り。
fn build_y_labels(y_max: f64) -> Vec<slint::SharedString> {
    (0..=6).map(|i| format!("{}h", (y_max * (6 - i) as f64 / 6.0).round() as i64).into()).collect()
}

// WeeklyChart.tsx のy2軸（入眠・起床の時刻軸）相当。7段階で均等割りし、時刻ラベルに変換する。
fn build_y2_labels(y2_min: f64, y2_max: f64) -> Vec<slint::SharedString> {
    (0..=6).map(|i| {
        let v = y2_max - (y2_max - y2_min) * i as f64 / 6.0;
        format!("{:02}:00", (v.floor() as i64).rem_euclid(24)).into()
    }).collect()
}

// Catmull-Romスプラインで2点(p1,p2)間をなめらかに補間する（p0,p3は前後の制御点）。
fn catmull_rom(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let calc = |a: f32, b: f32, c: f32, d: f32| -> f32 {
        0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (calc(p0.0, p1.0, p2.0, p3.0), calc(p0.1, p1.1, p2.1, p3.1))
}

// (x位置, y値)のペア（Noneはデータ欠損）から、連続する区間ごとにCatmull-Rom
// スプラインでサンプル点を密に生成する。折れ線を「曲線・実線」に見せるため、
// Slint側ではこのサンプル点を小さい点として大量に並べて描画する。
// x位置は呼び出し側が指定する（通常の7日は(i+0.5)/7、週境界をまたぐ隣接日
// スタブ線分は前週日曜=-0.5/7・翌週月曜=7.5/7を渡す。0.0〜1.0の外に出た部分は
// main.slint側でクリップして「グラフの端で切れる」ように見せる）。
fn build_curve(points: &[Option<(f32, f32)>]) -> Vec<CurvePointVM> {
    const STEPS: usize = 28;

    let mut result = Vec::new();
    let mut run: Vec<(f32, f32)> = Vec::new();
    let flush = |run: &mut Vec<(f32, f32)>, result: &mut Vec<CurvePointVM>| {
        let n = run.len();
        let start_len = result.len();
        if n == 1 {
            result.push(CurvePointVM { x: run[0].0, y: run[0].1, new_run: true });
        } else if n > 1 {
            for i in 0..n - 1 {
                let p0 = if i == 0 { run[0] } else { run[i - 1] };
                let p1 = run[i];
                let p2 = run[i + 1];
                let p3 = if i + 2 < n { run[i + 2] } else { run[n - 1] };
                for s in 0..STEPS {
                    let t = s as f32 / STEPS as f32;
                    let (x, y) = catmull_rom(p0, p1, p2, p3, t);
                    result.push(CurvePointVM { x, y, new_run: false });
                }
            }
            result.push(CurvePointVM { x: run[n - 1].0, y: run[n - 1].1, new_run: false });
        }
        if let Some(first) = result.get_mut(start_len) {
            first.new_run = true;
        }
        run.clear();
    };
    for p in points {
        match p {
            Some(pt) => run.push(*pt),
            None => flush(&mut run, &mut result),
        }
    }
    flush(&mut run, &mut result);
    clip_to_unit_x(result)
}

// 週境界をまたぐスタブ線分（x<0 または >1）を、x=0.0〜1.0の境界との交点で
// 正確に切る（単純に座標をクランプするとy座標だけ動いた「縦につぶれた線」に
// なってしまうため、線分と境界の交点を線形補間で求めて切断する）。
// Slint側のclip:trueによるクリップは、Android実機のレンダリングバックエンドで
// 効かない（線が描画領域の外まで突き抜けてしまう）ことが実機検証で判明したため、
// プラットフォーム差異が出ないようRust側で完結させる。
fn clip_to_unit_x(points: Vec<CurvePointVM>) -> Vec<CurvePointVM> {
    let mut out: Vec<CurvePointVM> = Vec::with_capacity(points.len());
    let mut prev: Option<(f32, f32, bool)> = None; // (x, y, 直前の点が範囲内だったか)
    for p in points {
        if p.new_run {
            prev = None;
        }
        let cur_in = p.x >= 0.0 && p.x <= 1.0;
        match prev {
            None => {
                if cur_in {
                    out.push(CurvePointVM { x: p.x, y: p.y, new_run: true });
                }
            }
            Some((px, py, was_in)) => {
                if was_in && cur_in {
                    out.push(CurvePointVM { x: p.x, y: p.y, new_run: false });
                } else if was_in && !cur_in {
                    // 範囲外へ出る: 境界との交点で切って終える
                    let boundary = if p.x > 1.0 { 1.0 } else { 0.0 };
                    let t = (boundary - px) / (p.x - px);
                    out.push(CurvePointVM { x: boundary, y: py + t * (p.y - py), new_run: false });
                } else if !was_in && cur_in {
                    // 範囲内へ入る: 境界との交点から新しいランとして始める
                    let boundary = if px < 0.0 { 0.0 } else { 1.0 };
                    let t = (boundary - px) / (p.x - px);
                    out.push(CurvePointVM { x: boundary, y: py + t * (p.y - py), new_run: true });
                    out.push(CurvePointVM { x: p.x, y: p.y, new_run: false });
                }
                // 両方とも範囲外ならその区間は描画しない
            }
        }
        prev = Some((p.x, p.y, cur_in));
    }
    out
}

pub fn update_chart(window: &MainWindow, state: &SharedState) {
    let (week_base, selected, open_sleep_start) = {
        let s = state.lock().unwrap();
        (s.week_base, s.selected_date.clone(), s.open_sleep_start)
    };
    let sessions = events::get_sessions().unwrap_or_default();
    let excluded_dates = events::get_excluded_dates();
    let night_boundary = config::load_config_inner().night_type_boundary_hour
        .unwrap_or(config::NIGHT_TYPE_BOUNDARY_HOUR_DEFAULT);
    let days = utils::build_week(&sessions, week_base, &excluded_dates, night_boundary);

    // 週境界（日曜→月曜）をまたぐ昼夜逆転が一目で分かるよう、隣接週の境界日
    // （前週の日曜・翌週の月曜）の入眠/起床時刻も取得する。両方にデータがあれば
    // 折れ線をグラフの端までわずかに伸ばし、端で切れるように見せる
    // （build_curveでのx座標クランプと対応。詳細はbuild_curveのコメント参照）。
    let week_start_date = utils::week_start(week_base);
    let prev_day_summary = utils::single_day_summary(&sessions, week_start_date - chrono::Duration::days(1), &excluded_dates, night_boundary);
    let next_day_summary = utils::single_day_summary(&sessions, week_start_date + chrono::Duration::days(7), &excluded_dates, night_boundary);

    // 進行中（まだIDLE_RESUMEが来ていない）セッションがこの週のどこかにあれば、
    // その開始日のバーとして扱う（完了済みセッションの日付バケット判定と同じ、
    // 開始時刻から導出した「睡眠日」で決める。build_weekと同じ基準）。
    let in_progress_hours = open_sleep_start.map(|start| {
        let elapsed_h = (chrono::Local::now().naive_local() - start).num_seconds() as f64 / 3600.0;
        (utils::sleep_day(start), elapsed_h.max(0.0))
    });

    let raw_max = days.iter().map(|d| d.total_hours)
        .chain(in_progress_hours.map(|(_, h)| h))
        .fold(0.0_f64, f64::max)
        .max(6.0);
    let y_max = raw_max.ceil() + 1.0;

    // y2軸（就寝/起床の時刻軸）は常に基準時刻(night_boundary)を上端・下端とする
    // 固定24h幅にする。to_night_hourの変換で値は必ず[night_boundary, night_boundary+24)
    // に収まるため、データに応じて範囲が動かない固定軸にできる（データが無い日・
    // 極端な時刻の日があってもグラフの見た目が変わらない）。
    let y2_min = night_boundary;
    let y2_max = night_boundary + 24.0;
    let y2_range = (y2_max - y2_min).max(0.001);
    // 折れ線グラフ用: 0.0(上端)〜1.0(下端)の縦位置に正規化する
    let y_frac = |v: f64| (1.0 - ((v - y2_min) / y2_range).clamp(0.0, 1.0)) as f32;

    let vm: Vec<DaySummaryVM> = days.iter().enumerate().map(|(i, d)| {
        let date_str = d.date.format("%Y-%m-%d").to_string();
        let is_active = selected.as_deref() == Some(date_str.as_str());
        let in_progress = in_progress_hours.filter(|(day, _)| *day == d.date).map(|(_, h)| h);
        DaySummaryVM {
            date: date_str.into(),
            day_label: format!("{} {}/{}", DAYS_JA[i], d.date.month(), d.date.day()).into(),
            duration_label: if d.total_hours > 0.0 { utils::format_duration(d.total_hours).into() } else { "".into() },
            bar_frac: (d.total_hours / y_max) as f32,
            has_data: d.total_hours > 0.0,
            active: is_active,
            bedtime_y: d.bedtime_h.map(y_frac).unwrap_or(0.0),
            bedtime_has: d.bedtime_h.is_some(),
            waketime_y: d.waketime_h.map(y_frac).unwrap_or(0.0),
            waketime_has: d.waketime_h.is_some(),
            in_progress_frac: (in_progress.unwrap_or(0.0) / y_max) as f32,
            in_progress_has: in_progress.is_some(),
            in_progress_label: in_progress.map(utils::format_duration).unwrap_or_default().into(),
            excluded: d.excluded,
        }
    }).collect();
    // 通常の7日ぶんに加え、先頭に前週日曜（x=-0.5/7）・末尾に翌週月曜（x=7.5/7）の
    // 点を追加する。どちらもデータが無ければNoneのままなので、build_curveの
    // 通常のラン分割ロジックにより余計な線分は描かれない
    // （隣接する境界日どうしにデータが揃った時だけ繋がる）。
    let bedtime_pts: Vec<Option<(f32, f32)>> = std::iter::once(prev_day_summary.bedtime_h.map(|h| (-0.5 / 7.0, y_frac(h))))
        .chain(days.iter().enumerate().map(|(i, d)| d.bedtime_h.map(|h| ((i as f32 + 0.5) / 7.0, y_frac(h)))))
        .chain(std::iter::once(next_day_summary.bedtime_h.map(|h| (7.5 / 7.0, y_frac(h)))))
        .collect();
    let waketime_pts: Vec<Option<(f32, f32)>> = std::iter::once(prev_day_summary.waketime_h.map(|h| (-0.5 / 7.0, y_frac(h))))
        .chain(days.iter().enumerate().map(|(i, d)| d.waketime_h.map(|h| ((i as f32 + 0.5) / 7.0, y_frac(h)))))
        .chain(std::iter::once(next_day_summary.waketime_h.map(|h| (7.5 / 7.0, y_frac(h)))))
        .collect();
    window.set_bedtime_curve(slint::ModelRc::new(slint::VecModel::from(build_curve(&bedtime_pts))));
    window.set_waketime_curve(slint::ModelRc::new(slint::VecModel::from(build_curve(&waketime_pts))));

    window.set_week(slint::ModelRc::new(slint::VecModel::from(vm)));

    window.set_y_labels(slint::ModelRc::new(slint::VecModel::from(build_y_labels(y_max))));
    window.set_y2_labels(slint::ModelRc::new(slint::VecModel::from(build_y2_labels(y2_min, y2_max))));

    let fmt = |d: NaiveDate| format!("{}/{:02}/{:02} ({})", d.year(), d.month(), d.day(), DAYS_JA[d.weekday().num_days_from_monday() as usize]);
    let end = week_base + chrono::Duration::days(6 - week_base.weekday().num_days_from_monday() as i64);
    let start = week_base - chrono::Duration::days(week_base.weekday().num_days_from_monday() as i64);
    window.set_week_range_label(format!("{} 〜 {}", fmt(start), fmt(end)).into());
}
