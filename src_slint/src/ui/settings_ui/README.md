# src_slint/src/ui/settings_ui

`../settings_ui.rs`（エントリポイント）の実装を担う内部サブモジュール群。600行超だった
`settings_ui.rs`を責務ごとに分割している。外部からはこのフォルダの内部ファイルへ直接
アクセスせず、必ず`../settings_ui.rs`の再公開経由で使う。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `general.rs` | 起動設定・睡眠判定時間・目標起床時刻・クラウド連携の読み込み/保存、接続テスト、手動同期 |
| `export.rs` | CSVエクスポート（PC: rfdダイアログ / Android: 固定パス） |
| `backup_restore.rs` | 手動バックアップ・バックアップ一覧・一覧からの復元・外部ファイルピッカー経由の復元 |
| `data_management.rs` | ローカル/クラウド全データ削除・データ圧縮・バックアップ履歴削除（いずれも2回クリック確認） |

## 依存関係

- 依存する（import する）フォルダ：`../../core`、`../home`（`SharedState`/`refresh_all`）、`../../platform`（windowsモジュール）
- 依存される（import される）フォルダ：なし（`../settings_ui.rs`経由でのみ外部から使われる）

## 内部モジュール間の依存

共通の確認待ちフラグ（`*_CONFIRM_PENDING`）・メッセージ種別定数（`KIND_*`）・
`clear_stale_confirmations`/`now_hms`は`../settings_ui.rs`（エントリポイント）に定義し、
各サブモジュールから`super::`で参照する。
