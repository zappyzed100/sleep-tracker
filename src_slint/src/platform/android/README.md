# src_slint/src/platform/android

Android専用のプラットフォーム機能（Layer 1）。エントリポイント・フォアグラウンド定期同期・
バックアップ復元・アプリ使用状況のJNIブリッジをまとめる。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `mod.rs` | エントリポイント。外部から使う関数だけを再公開する |
| `entry.rs` | `android_main`（cargo-apkから呼ばれるAndroid版エントリポイント） |
| `bg.rs` | フォアグラウンド定期同期・`MainActivity.onResume()`からのJNI経由同期(`nativeOnResume`) |
| `restore.rs` | 「バックアップから復元」のファイルピッカー連携。KotlinのACTION_OPEN_DOCUMENTピッカーをJNIで起動し、選択結果を`nativeRestorePicked`で受け取る |
| `usage.rs` | 「睡眠判定に使うアプリ」のJNIブリッジ。UsageReporter.ktが検知したパッケージ名の記録・現在の許可状態の取得を`core::events`に橋渡しする |

## 依存関係

- 依存する（import する）フォルダ：`../../core`, `../../ui`（`bg.rs`/`restore.rs`/`usage.rs`から`home`/`sync_status`/`events`を利用）
- 依存される（import される）フォルダ：`../../`（lib.rs）、`../../ui`（settings_ui.rsから`launch_picker`を利用）

## 公開 API（mod.rs）

- `setup(window, state)` — フォアグラウンド定期同期の初期化（`bg`から再公開）
- `activity()` — MainActivityインスタンスへのJNIグローバル参照（`bg`から再公開）
- `refresh_ui()` — 同期を停止中でも動くUI再読み込み（`bg`から再公開）
- `launch_picker(weak, state)` — バックアップ復元のファイルピッカー起動（`restore`から再公開）

`entry.rs`と`usage.rs`はJNI（`#[no_mangle]`）経由でKotlin側から直接呼ばれるため、
Rust側のpub再公開は不要（内部モジュール `mod entry;` / `mod usage;` として存在するだけでよい）。
