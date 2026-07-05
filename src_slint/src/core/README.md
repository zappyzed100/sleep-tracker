# src_slint/src/core

ビジネスロジック層（Layer 0）。Tauri版 `src_tauri/src-tauri/src/` からほぼそのまま移植している。
OS固有のAPI（Windows/Android）やSlintのUI型に依存しない。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `mod.rs` | エントリポイント。`config`/`events`/`cloud`/`prediction`/`utils` を公開し、`Session` をcrate直下へ再公開する |
| `config.rs` | AppConfig読み書き・Drive設定同期 |
| `events.rs` | sleep_events.txtパース・セッションCRUD・CSV出力（テストは`events/events_tests.rs`） |
| `cloud.rs` | Google Apps Script経由のDrive同期・モバイルイベント取得 |
| `prediction.rs` | ランダムフォレストによる睡眠時間予測 |
| `utils.rs` | 表示用フォーマット・週データ構築 |

## 依存関係

- 依存する（import する）フォルダ：なし
- 依存される（import される）フォルダ：`../platform`, `../ui`, `../`（lib.rs）
