# src_slint/src/core/events

`events.rs` のテスト専用サブモジュール。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `events_tests.rs` | `parse_sessions_from_str`（睡眠セッション再構築ロジック）・`detect_open_idle_and_out`・`is_out_from_content`・`coalesce_and_filter_app_usage` の単体テスト。`#[cfg(test)]` のみでビルドされる |

## 依存関係

- 依存する（import する）フォルダ：なし（`super::*` で親モジュール `events.rs` の非公開関数を参照するのみ）
- 依存される（import される）フォルダ：なし（テストからのみ使われる）

## 注意

テストの入力データはすべて架空の日付（`2024-01-01`〜）を使う。実際のユーザーの
睡眠データ（日付・時刻）はプライバシー上コミットしないため、実データを再現する
回帰テストも実データの日付そのものではなく同じ「事象の形」を架空の日付で表現する。
