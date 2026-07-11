# src_slint/src/ui/home

`../home.rs`（エントリポイント）の実装を担う内部サブモジュール群。700行超だった
`home.rs`を責務ごとに分割している。外部からはこのフォルダの内部ファイルへ直接
アクセスせず、必ず`../home.rs`の再公開経由で使う。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `state.rs` | アプリ共有状態(AppState/SharedState)の定義、期間タブ(Period)、週送り・全体再読み込み(`refresh_all`) |
| `stats.rs` | 統計ストリップ・睡眠予測カードの計算とtick更新(`compute_stats`/`apply_tick`/`recompute_prediction`) |
| `chart.rs` | 週間チャート（軸ラベル生成・Catmull-Romスプライン曲線化・週境界クリップ、`update_chart`） |
| `calendar.rs` | カレンダーピッカー（月送り・日付タップによる週選択） |
| `day_detail.rs` | 日別詳細モーダル（セッション一覧表示・計測対象外への切り替え） |

## 依存関係

- 依存する（import する）フォルダ：`../../core`、`../../`(`MainWindow`等のVM型)
- 依存される（import される）フォルダ：なし（`../home.rs`経由でのみ外部から使われる）

## 内部モジュール間の依存

同じ`home`配下の兄弟モジュール間は`super::`で直接参照し合う（例: `stats.rs`/`calendar.rs`/
`day_detail.rs`はいずれも`chart::update_chart`を呼ぶ、`day_detail.rs`は`stats::compute_stats`も呼ぶ）。
`AppState`のフィールドは`pub(super)`で`home`配下全体からアクセスできるようにしている
（外部からは`SharedState`型を通じてのみ扱う設計は変えていない）。
