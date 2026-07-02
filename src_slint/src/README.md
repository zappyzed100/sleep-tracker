# src_slint/src

Rustソース一式。現在14ファイルとCLAUDE.mdの目安（7ファイル）を超えているため、
将来的に `core/`（config/events/cloud/prediction/utils）・`platform/`（monitor/platform/tray/android_bg）・
`ui/`（home/settings_ui）へのサブフォルダ分割を検討する（未着手）。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `lib.rs` | エントリポイント共通ロジック。共有static・パスユーティリティ・全コールバック配線 |
| `main.rs` | デスクトップ版エントリポイント（`lib.rs::run()` を呼ぶだけ） |
| `android.rs` | Android版エントリポイント（`android_main`、cargo-apkから呼ばれる） |
| `android_bg.rs` | Android版フォアグラウンド定期同期・起動時DEVICE_ON記録 |
| `config.rs` | AppConfig読み書き・Drive設定同期（Tauri版から移植） |
| `events.rs` | sleep_events.txtパース・セッションCRUD・CSV出力（Tauri版から移植） |
| `cloud.rs` | Google Apps Script経由のDrive同期・モバイルイベント取得（Tauri版から移植） |
| `prediction.rs` | ランダムフォレストによる睡眠時間予測（Tauri版から変更なしで移植） |
| `monitor.rs` | Windowsアイドル検知バックグラウンドスレッド（Tauri版から移植） |
| `platform.rs` | スタートアップ登録・デスクトップショートカット（Tauri版から移植） |
| `tray.rs` | Windowsシステムトレイ（閉じるボタンで常駐） |
| `utils.rs` | 表示用フォーマット・週データ構築（core/utils.ts相当） |
| `home.rs` | ホーム画面（統計・予測・チャート・日別詳細）のUI連携 |
| `settings_ui.rs` | 設定画面のUI連携 |

## 依存関係

- 依存する（import する）フォルダ：`../ui`（`main.slint` をbuild.rs経由でコンパイル）
- 依存される（import される）フォルダ：なし
