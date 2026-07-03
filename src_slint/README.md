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
| `android/` | Gradle + cargo-ndk 構成のAndroidプロジェクト（WorkManager真バックグラウンド同期用、後述） |

## ビルド方法

### デスクトップ（Windows）

```bash
cargo build --release
./target/release/sleep_tracker.exe
```

### Android（2種類のビルド経路がある）

#### ① Gradle + cargo-ndk（`android/`、真バックグラウンド同期あり・配布用に推奨）

WorkManagerで15分ごとにDrive宛シグナルを送信するKotlin製 `DriveSignalWorker` を含み、
アプリがバックグラウンド／終了後でも動作する。実機に配布する場合はこちらを使う。

```bash
# 1. RustのcdylibをNDK向けにビルドし、android/app/src/main/jniLibs/ に配置
export ANDROID_HOME=... ANDROID_NDK_HOME=...
cargo ndk -t arm64-v8a -P 34 -o android/app/src/main/jniLibs build --release

# 2. Gradleでapkをビルド（毎回 clean を挟むこと。増分ビルドだと過去のjniLibsが
#    zipの空き領域に残り、apkサイズが異常に膨らむことがある）
cd android
export ANDROID_HOME=... JAVA_HOME=...
./gradlew.bat clean assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`-P 34`（プラットフォームAPIレベル）は、インストール済みのAndroid SDKに存在するレベルを指定する
必要がある（`sdkmanager --list` や `platforms/` ディレクトリで確認）。指定しないとデフォルトの21が
使われ、21のSDKプラットフォームが未インストールだと "No Android platforms found" で失敗する。

`android/local.properties`（sdk.dir）と `app/src/main/jniLibs/`（cargo-ndkの出力）はビルド生成物
のためgitignore対象。

#### ② cargo-apk（純Rust、素早い反復用）

WorkManagerが不要なUI/ロジック側の動作確認はこちらが手早い。Kotlinコードは含まれないため、
真のバックグラウンド同期（15分ごとのDEVICE_ON送信）は動かない。

```bash
# 初回のみ: cargo install cargo-apk
export ANDROID_HOME=... ANDROID_NDK_ROOT=... JAVA_HOME=...
cargo apk build --target aarch64-linux-android --lib   # 実機
cargo apk build --target x86_64-linux-android --lib    # エミュレータ
adb install target/debug/apk/sleep_tracker.apk
```

## 開発中の暫定事項

- Windowsインストーラー（MSI/NSIS）は未整備（`cargo-wix` で原因不明のエラーが発生中、保留）。
- CSVインポート機能は対象外（意図的にスコープ外）。
- Android版のCSVエクスポート/バックアップ/リストアは実装済み（rfdクレートがAndroidバックエンドを
  持たないため、外部ストレージ固定パスへの読み書きで代替）。
- Android版のリリース署名（keystore）は未整備。配布中のapkはdebug署名。

## 依存関係

- 依存する（import する）フォルダ：なし（`legacy/src_tauri` はコード移植元として参照したのみ）
- 依存される（import される）フォルダ：なし
