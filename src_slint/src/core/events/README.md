# src_slint/src/core/events

`../events.rs`（エントリポイント）の実装を担う内部サブモジュール群。1000行超だった
`events.rs`を責務ごとに分割している。外部からはこのフォルダの内部ファイルへ直接
アクセスせず、必ず`../events.rs`の再公開経由で使う。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `parsing.rs` | イベント行→睡眠セッション変換のステートマシン本体。ファイルソート・モバイルイベント取り込み・キャッシュ付き取得(`get_sessions`) |
| `excluded.rs` | 計測対象外の日（`DAY_EXCLUDED`/`DAY_INCLUDED`）の読み書き |
| `session_ops.rs` | 手動セッションの追加・削除（CRUD） |
| `backup.rs` | バックアップ・復元・全削除・データ圧縮(`compact_data`)・進行中セッション検出 |
| `csv.rs` | CSVエクスポート・インポート |
| `tests/` | 上記各モジュールの単体テスト。詳細は`tests/README.md`参照 |

## 依存関係

- 依存する（import する）フォルダ：`../../` (`crate::core::cloud`、`crate::core::utils`)
- 依存される（import される）フォルダ：なし（`../events.rs`経由でのみ外部から使われる）

## 内部モジュール間の依存

同じ`events`配下の兄弟モジュール間は`super::`または`crate::core::events::`で
直接参照し合う（例: `excluded.rs`/`session_ops.rs`/`backup.rs`はいずれも
`parsing::sort_events_file`等を使う）。`parsing.rs`は`excluded.rs`の
`excluded_dates_from_content`を使う（除外判定はセッション構築時に行うため）。
