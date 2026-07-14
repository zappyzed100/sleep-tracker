# src_slint/src/core/prediction

睡眠時間・最適入眠時刻の予測、および睡眠周期の検出を担う。`crate::core::prediction`
としてクレート直下から参照される（エントリポイントは`mod.rs`）。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `mod.rs` | ランダムフォレスト回帰による予測睡眠時間・最適入眠時刻の計算(`predict`/`find_optimal`) |
| `cycle.rs` | カイ二乗ペリオドグラムによる睡眠周期の検出(`estimate_sleep_cycle`) |

## 依存関係

- 依存する（import する）フォルダ：`crate::Session`、`../utils`（`cycle.rs`が
  `sleep_day`/`merge_intervals`/`SLEEP_DAY_BOUNDARY_HOUR`を使用）
- 依存される（import される）フォルダ：`../../ui/home`（`stats.rs`が
  `predict`/`find_optimal`/`estimate_sleep_cycle`を呼ぶ）

## 公開API

- `predict(sessions, now) -> PredictionResult` — 予測睡眠時間
- `find_optimal(sessions, now, target_wake_hhmm) -> Option<OptimalResult>` — 最適入眠時刻
- `estimate_sleep_cycle(sessions, excluded_dates) -> Option<CycleResult>` — 睡眠周期（時間）。
  データ不足・周期不明瞭な場合は`None`
