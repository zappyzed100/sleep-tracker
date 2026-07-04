# src_slint/src/ui

画面ロジック層（Layer 1）。`ui/main.slint` で定義したUIコンポーネントへのデータバインディングと
コールバック処理を担当する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `mod.rs` | エントリポイント。`home`/`settings_ui`/`sync_status` を公開する |
| `home.rs` | ホーム画面（統計・睡眠予測・週間チャート・日別詳細）のUI連携 |
| `settings_ui.rs` | 設定画面（起動設定・クラウド連携・データ管理）のUI連携 |
| `sync_status.rs` | 同期中インジケーター(sync-in-progress)の参照カウント管理。複数の同期処理が重なっても正しく回転・停止させる |

## 依存関係

- 依存する（import する）フォルダ：`../core`, `../platform`（windowsモジュールのみ）
- 依存される（import される）フォルダ：`../`（lib.rs）、`../platform`（monitor/android_bgからsync_statusを利用）
