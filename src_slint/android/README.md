# src_slint/android

Gradle + cargo-ndk 構成のAndroidプロジェクト。WorkManagerによる真のバックグラウンド同期
（`DriveSignalWorker`）を実現するために新設した。cargo-apk（`src_slint/Cargo.toml` の
`[package.metadata.android]`）はNativeActivityベースの純Rust構成でKotlin/Javaソースを
同梱できないため、Kotlinが必要なこの機能だけ別ビルド経路に分離している。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `settings.gradle.kts` | ルートプロジェクト設定（`:app` モジュールを含める） |
| `build.gradle.kts` | ルートビルドスクリプト。AGP 8.11.0 / Kotlin Gradle Plugin 2.1.0 |
| `gradle.properties` | AndroidX有効化などのGradleプロパティ |
| `gradle/wrapper/` | Gradle Wrapper本体（distributionは9.1.0） |
| `gradlew` / `gradlew.bat` | Gradle Wrapper起動スクリプト |
| `app/build.gradle.kts` | アプリモジュールのビルド設定（compileSdk/依存関係） |
| `app/src/main/AndroidManifest.xml` | マニフェスト（`MainActivity` 起動、INTERNET権限） |
| `app/src/main/res/values/strings.xml` | アプリ名などの文字列リソース |
| `app/src/main/kotlin/.../MainActivity.kt` | `NativeActivity` 継承。起動時にWorkManagerへ`DriveSignalWorker`の定期実行を登録 |
| `app/src/main/kotlin/.../DriveSignalWorker.kt` | 15分ごとにDrive宛シグナルをPOSTする`CoroutineWorker`（Tauri版から近い形で移植） |

`app/src/main/jniLibs/`（cargo-ndkのビルド出力）と `local.properties`（SDKパス）はビルド生成物
のため `.gitignore` で除外している。

## ビルド手順

```bash
# 1. RustのcdylibをNDK向けにビルドし、jniLibs/ に配置
export ANDROID_HOME=... ANDROID_NDK_HOME=...
cd src_slint
cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build --release

# 2. Gradleでapkをビルド
cd android
export ANDROID_HOME=... JAVA_HOME=...
./gradlew.bat assembleDebug

# 3. インストール（既にcargo-apk版が入っている場合はversionCode不一致で
#    INSTALL_FAILED_VERSION_DOWNGRADEになるため先にアンインストール）
adb uninstall com.sleeptracker.app
adb install app/build/outputs/apk/debug/app-debug.apk
```

`local.properties` は各自の環境で作成する（`sdk.dir=<Android SDKのパス>`）。

## Gradle/JDKバージョンについて

Gradle 8.14.3付属のKotlinコンパイラはJDK 25のバージョン文字列を解析できずビルドスクリプトの
コンパイルに失敗する。この問題はプロジェクト側のKotlin Gradle Pluginを上げても直らず、
Gradle Wrapper自体を9.1.0（JDK25公式対応）に上げることで解決した。

`androidx.work:work-runtime-ktx:2.10.1` は `compileSdk >= 35` を要求するため、
`app/build.gradle.kts` の `compileSdk` は34ではなく36にしている。

## MainActivity.kt / DriveSignalWorker.kt の設計方針

Tauri版（`legacy/src_tauri/src-tauri/gen/android/.../MainActivity.kt` および
`DriveSignalWorker.kt`）のWorkManager登録ロジックをほぼそのまま移植した。ただし
Slint版はWebViewを使わないため、Tauri版にあった以下の要素は移植していない：

- `AppBridge` JSインターフェース（`setTab`/`notifyReady`）
- ハードウェア戻るボタンのカスタム処理
- 黒画面/WebViewフリーズ対策（`recreate()` ワークアラウンド） — WebView自体を使わないため
  この不具合クラスが構造的に発生しない

## 依存関係

- 依存する（import する）フォルダ：なし（Rust側の `cargo ndk` ビルド出力である
  `jniLibs/*.so` を実行時に読み込むが、ソース上のimportではない）
- 依存される（import される）フォルダ：なし
