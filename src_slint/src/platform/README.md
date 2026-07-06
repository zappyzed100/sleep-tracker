# src_slint/src/platform

OS固有機能層（Layer 1）。Windows（起動設定・アイドル監視・システムトレイ）とAndroid
（エントリポイント・フォアグラウンド同期）を担当する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `mod.rs` | エントリポイント。各モジュールをcfgで出し分けて公開する |
| `windows.rs` | スタートアップ登録・デスクトップショートカット（全プラットフォームから呼ばれるが中身はWindows専用） |
| `monitor.rs` | Windowsアイドル検知バックグラウンドスレッド（Windowsのみ） |
| `tray.rs` | Windowsシステムトレイ（閉じるボタンで常駐、Windowsのみ） |
| `android.rs` | Android版エントリポイント（`android_main`、cargo-apkから呼ばれる、Androidのみ） |
| `android_bg.rs` | Android版フォアグラウンド定期同期・`MainActivity.onResume()`からのJNI経由同期(`nativeOnResume`)（Androidのみ） |
| `android_restore.rs` | 「バックアップから復元」のファイルピッカー連携。KotlinのACTION_OPEN_DOCUMENTピッカーをJNIで起動し、選択結果を`nativeRestorePicked`で受け取る（Androidのみ） |
| `android_usage.rs` | 「睡眠判定に使うアプリ」のJNIブリッジ。UsageReporter.ktが検知したパッケージ名の記録・現在の許可状態の取得を`core::events`に橋渡しする（Androidのみ） |

## 依存関係

- 依存する（import する）フォルダ：`../core`, `../ui`（`monitor.rs`/`android_bg.rs`/`android_restore.rs`/`android_usage.rs`から`home`/`sync_status`/`events`を利用）
- 依存される（import される）フォルダ：`../`（lib.rs）

## 注意

`platform/`直下が8ファイル（README除く）になっている。Android専用の4ファイル
（`android.rs`/`android_bg.rs`/`android_restore.rs`/`android_usage.rs`）を
`platform/android/`サブフォルダへまとめる分割は今後の課題とする。
