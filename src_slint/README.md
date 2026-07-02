# src_slint

Rust + Slint で実装している新版アプリ（PC・Android共通コードベース）。Tauri版（`legacy/src_tauri`）の後継。

## 背景

Tauri（Rust + React/TypeScript + WebView）から、UIも含めてRustのみで完結するSlintへ移行中。
WebViewを使わないため、Tauri版で問題になっていたAndroidのWebViewフリーズ・黒画面系の不具合が
構造的に発生しない。ビジネスロジック（`config.rs`, `events.rs`, `cloud.rs`, `prediction.rs`,
`monitor.rs`, `platform.rs`）はTauri版からほぼそのまま移植している。

## 含まれるフォルダ・ファイル

| フォルダ / ファイル | 役割 |
|---------------------|------|
| `src/` | Rustソース一式（詳細は `src/README.md` 参照） |
| `ui/main.slint` | 全UI定義（ホーム画面・設定画面・日別詳細モーダル） |
| `assets/` | トレイアイコン等の画像アセット |
| `build.rs` | ビルド時に `ui/main.slint` をコンパイルする |
| `Cargo.toml` | lib+bin構成。`[lib]` は cdylib+rlib（Android用）、デスクトップは `src/main.rs` |

## ビルド方法

### デスクトップ（Windows）

```bash
cargo build --release
./target/release/sleep_tracker.exe
```

### Android

```bash
# 初回のみ: cargo install cargo-apk
export ANDROID_HOME=... ANDROID_NDK_ROOT=... JAVA_HOME=...
cargo apk build --target aarch64-linux-android --lib   # 実機
cargo apk build --target x86_64-linux-android --lib    # エミュレータ
adb install target/debug/apk/sleep_tracker.apk
```

## 開発中の暫定事項

- `data_dir()` はデスクトップ版では `../src_tauri/data/` を共有している（実データでの検証用）。
  Tauri版を廃止する際は独立したデータディレクトリに切り替える必要がある。
- Windowsインストーラー（MSI/NSIS）は未整備（`cargo-wix` で原因不明のエラーが発生中、保留）。
- CSVインポート・Android版のCSVエクスポート/バックアップ/リストアは未実装（rfdクレートが
  Androidバックエンドを持たないため）。
- Android版の同期はアプリ起動中のみ（5分ごとのタイマー）。Tauri版のDriveSignalWorker相当の
  真のバックグラウンド同期（アプリ終了後もWorkManagerで動く）にはKotlin側のWorkerクラスが
  必要になり、cargo-apkの純Rust構成の範囲外のため未実装。

## 依存関係

- 依存する（import する）フォルダ：なし（`legacy/src_tauri` はコード移植元として参照したのみ）
- 依存される（import される）フォルダ：なし
