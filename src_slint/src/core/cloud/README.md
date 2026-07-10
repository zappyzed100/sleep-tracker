# src_slint/src/core/cloud

`../cloud.rs`（エントリポイント）の実装を担う内部サブモジュール群。900行超だった
`cloud.rs`を責務ごとに分割している。外部からはこのフォルダの内部ファイルへ直接
アクセスせず、必ず`../cloud.rs`の再公開経由で使う。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `generation.rs` | 世代番号ガード（全削除・圧縮の伝播判定）と`merge_into_local`（Drive⇔ローカルのunionマージ） |
| `validation.rs` | Driveから取得した内容のクライアント側検証（HTML/JS混入検知・イベント行形式チェック） |
| `backup_drive.rs` | sleep_events.txt/sleep_manual.txtのDriveへのpush（ハッシュベースの楽観的並行性制御込み） |
| `fetch.rs` | モバイルイベント取得(`pull_mobile_events_inner`)・Drive内容取得・接続テスト |
| `sync.rs` | Drive⇔ローカルの同期オーケストレーション（`sync_gist`/`sync_mobile_inner`/`auto_backup_after_event`等） |
| `clear_data.rs` | クラウド全削除・圧縮後の正データ直接反映 |
| `cloud_tests.rs` | `merge_into_local`・`merge_or_adopt_at`・`looks_like_html_or_js`・`looks_like_events_content`の単体テスト。`#[cfg(test)]`のみでビルドされる |

## 依存関係

- 依存する（import する）フォルダ：`../` (`crate::core::config`、`crate::core::events`)
- 依存される（import される）フォルダ：なし（`../cloud.rs`経由でのみ外部から使われる）

## 内部モジュール間の依存

同じ`cloud`配下の兄弟モジュール間は`super::`で直接参照し合う（例: `sync.rs`は
`generation`/`fetch`/`backup_drive`のほぼ全関数を組み合わせて使う、`fetch.rs`は
`validation`の検証関数を使う）。テストの入力データはすべて架空の日付
（`2024-01-01`〜）を使う。
