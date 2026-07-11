# src_slint/src/platform/android

Android専用のプラットフォーム機能（Layer 1）。エントリポイント・フォアグラウンド定期同期・
バックアップ復元をまとめる。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `mod.rs` | エントリポイント。外部から使う関数だけを再公開する |
| `entry.rs` | `android_main`（cargo-apkから呼ばれるAndroid版エントリポイント） |
| `bg.rs` | フォアグラウンド定期同期・`MainActivity.onResume()`からのJNI経由同期(`nativeOnResume`) |
| `restore.rs` | 「バックアップから復元」のファイルピッカー連携。KotlinのACTION_OPEN_DOCUMENTピッカーをJNIで起動し、選択結果を`nativeRestorePicked`で受け取る |

## 依存関係

- 依存する（import する）フォルダ：`../../core`, `../../ui`（`bg.rs`/`restore.rs`から`home`/`sync_status`/`events`を利用）
- 依存される（import される）フォルダ：`../../`（lib.rs）、`../../ui`（settings_ui.rsから`launch_picker`を利用）

## 公開 API（mod.rs）

- `setup(window, state)` — フォアグラウンド定期同期の初期化（`bg`から再公開）
- `activity()` — MainActivityインスタンスへのJNIグローバル参照（`bg`から再公開）
- `launch_picker(weak, state)` — バックアップ復元のファイルピッカー起動（`restore`から再公開）

`entry.rs`はJNI（`#[no_mangle]`）経由でKotlin側から直接呼ばれるため、
Rust側のpub再公開は不要（内部モジュール `mod entry;` として存在するだけでよい）。

## 注意

画面ON区間(SCREEN_ON_START/SCREEN_ON_END)の検出・送信はKotlin側の`UsageReporter.kt`が
HTTP POST直送信で完結しており、JNI経由のブリッジは不要（旧「睡眠判定に使うアプリ」機能の
JNIブリッジ`usage.rs`は廃止した）。
