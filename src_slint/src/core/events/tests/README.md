# src_slint/src/core/events/tests

`../`（`core/events/`）配下の各実装ファイルに対応する単体テスト。実装ファイルごとに
1:1でテストファイルを分け、実装本体が500行制限に収まるよう分離している。
`#[cfg(test)]`のみでビルドされる。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `parsing_tests.rs` | `parsing.rs`のテスト。`parse_sessions_from_str`（睡眠セッション再構築ロジック）・`is_out_from_content`・`coalesce_and_filter_screen_on` |
| `excluded_tests.rs` | `excluded.rs`のテスト。`excluded_dates_from_content`と、その結果が`parse_sessions_from_str`のSession.excludedに反映されるかの統合的な検証 |
| `backup_tests.rs` | `backup.rs`のテスト。`detect_open_idle_and_out`・`extract_preserved_metadata_lines` |

## 依存関係

- 依存する（import する）フォルダ：`../`（`core::events::{parsing, excluded, backup}`の`pub(super)`関数を直接参照する）
- 依存される（import される）フォルダ：なし（テストからのみ使われる）

## 注意

テストの入力データはすべて架空の日付（`2024-01-01`〜）を使う。実際のユーザーの
睡眠データ（日付・時刻）はプライバシー上コミットしないため、実データを再現する
回帰テストも実データの日付そのものではなく同じ「事象の形」を架空の日付で表現する。
