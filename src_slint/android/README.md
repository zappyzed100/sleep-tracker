# src_slint/android

Gradle + cargo-ndk 構成のAndroidプロジェクト。タブレット利用区間の検出送信に
Kotlinが必要なため、cargo-apk（`src_slint/Cargo.toml` の
`[package.metadata.android]`、NativeActivityベースの純Rust構成でKotlin/Javaソースを
同梱できない）とは別ビルド経路に分離している。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| `settings.gradle.kts` | ルートプロジェクト設定（`:app` モジュールを含める） |
| `build.gradle.kts` | ルートビルドスクリプト。AGP 8.11.0 / Kotlin Gradle Plugin 2.1.0 |
| `gradle.properties` | AndroidX有効化などのGradleプロパティ |
| `gradle/wrapper/` | Gradle Wrapper本体（distributionは9.1.0） |
| `gradlew` / `gradlew.bat` | Gradle Wrapper起動スクリプト |
| `app/build.gradle.kts` | アプリモジュールのビルド設定（compileSdk/依存関係） |
| `app/src/main/AndroidManifest.xml` | マニフェスト（`MainActivity` 起動、INTERNET・バッテリー最適化除外・使用状況アクセス権限） |
| `app/src/main/res/values/strings.xml` | アプリ名などの文字列リソース |
| `app/src/main/kotlin/.../MainActivity.kt` | `NativeActivity` 継承。起動時にバッテリー最適化除外・使用状況アクセスを要求し、再開のたびに画面ON区間の回収送信・Rust側同期(`nativeOnResume`)を行う |
| `app/src/main/kotlin/.../UsageReporter.kt` | `UsageStatsManager`からタブレットの画面が実際にONだった区間を検出し、`SCREEN_ON_START`/`SCREEN_ON_END`としてDriveへ送信する |

`app/src/main/jniLibs/`（cargo-ndkのビルド出力）と `local.properties`（SDKパス）はビルド生成物
のため `.gitignore` で除外している。

## ビルド手順

```bash
# 1. RustのcdylibをNDK向けにビルドし、jniLibs/ に配置（実機はarm64-v8aのみで十分）
export ANDROID_HOME=... ANDROID_NDK_HOME=...
cd src_slint
cargo ndk -t arm64-v8a -P 34 -o android/app/src/main/jniLibs build --release

# 2. デバッグシンボルを手動でstrip（Gradle側のstripは失敗することが多く、
#    しないとAPKが極端に肥大化する）
llvm-strip --strip-unneeded -o out.so android/app/src/main/jniLibs/arm64-v8a/libsleep_tracker.so
mv out.so android/app/src/main/jniLibs/arm64-v8a/libsleep_tracker.so

# 3. Gradleでapkをビルド（毎回 clean を挟むこと。増分ビルドだと過去のjniLibsが
#    zipの空き領域に残り、apkサイズが異常に膨らむことがある）
cd android
export ANDROID_HOME=... JAVA_HOME=...
./gradlew.bat clean assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`-P 34`（プラットフォームAPIレベル）は、インストール済みのAndroid SDKに存在するレベルを指定する
必要がある（`sdkmanager --list` や `platforms/` ディレクトリで確認）。指定しないとデフォルトの21が
使われ、21のSDKプラットフォームが未インストールだと "No Android platforms found" で失敗する。

`local.properties` は各自の環境で作成する（`sdk.dir=<Android SDKのパス>`）。

## Gradle/JDKバージョンについて

Gradle 8.14.3付属のKotlinコンパイラはJDK 25のバージョン文字列を解析できずビルドスクリプトの
コンパイルに失敗する。この問題はプロジェクト側のKotlin Gradle Pluginを上げても直らず、
Gradle Wrapper自体を9.1.0（JDK25公式対応）に上げることで解決した。

（`compileSdk`は36のままにしているが、現在の依存関係だけなら34でも足りる。下げる積極的な
理由もないため据え置いている。）

## MainActivity.kt / UsageReporter.kt の設計方針

タブレットが「電源ON」であることと「実際に使われていたか」は別物、という判断のもとで
以下のように役割を分離している（詳細な設計検証は開発時に別途プロトタイプで確認した）：

- **DriveSignalWorker（旧: アプリを開くたびのDEVICE_ON即時送信）**: 完全に廃止した。
  当初は15分ごとの定期実行として存在し、それも「タブレットの電源が入っているか」しか
  分からず信頼できないと判断して一回限りの実行に縮小していたが、その後
  「未クローズの睡眠セッションを、その後最初に来たDEVICE_ONで閉じる」という
  Rust側ロジックが、夜間の無関係なDEVICE_ONを拾って実際の睡眠時間より大幅に短い
  セッションを捏造するバグを引き起こしたため、DEVICE_ON自体の送信・記録をやめた
  （夜中に何度もアプリを開いて確認する使い方だと、ログが無駄に増えるだけの
  副作用しかなかった）。
- **APP_USAGE_START/END（旧: アプリ単位のフォアグラウンド検出）**: 廃止した。
  `MOVE_TO_FOREGROUND`/`MOVE_TO_BACKGROUND`（どのアプリがアクティブか）を使っていたが、
  画面が自動消灯した後もOS・機種（MIUI等）によっては直前のアプリが「まだフォアグラウンド
  扱い」のまま残り続け、次にアプリを切り替えるまで`MOVE_TO_BACKGROUND`が発生しない
  バグがあった。その結果、画面OFFで何時間も放置していた時間まで丸ごと利用区間として
  記録されてしまっていた。あわせて、どのアプリを対象にするかのアプリ単位許可リスト
  （`USAGE_APP_SEEN`/`ALLOWED`/`DENIED`、設定画面の「睡眠判定に使うアプリ」）も廃止した。
- **UsageReporter（SCREEN_ON_START/END）**: `UsageStatsManager`から取得できる
  `SCREEN_INTERACTIVE`/`SCREEN_NON_INTERACTIVE`（画面が実際にON/OFFになった実イベント、
  API 28以降）を使い、画面が実際にONだった区間を`SCREEN_ON_START`/`SCREEN_ON_END`として
  送信する。アプリ単位の識別は行わない。在宅解除も`SCREEN_ON_START`（画面が実際にONに
  なった証拠）だけに一本化している。OS側の履歴は常時記録されているため、こちらのアプリが
  バックグラウンドで動いている必要がない。アプリを開くたびに前回確認時点からの履歴を
  まとめて回収するだけでよい。短すぎる点灯の除外・近接する点灯の統合はRust側
  （`core/events/parsing.rs`の`coalesce_and_filter_screen_on`）が一括で担うため、Kotlin側に
  しきい値を重複させない。初回利用時に「使用状況へのアクセス」（`PACKAGE_USAGE_STATS`）の
  手動許可が必要（`SCREEN_INTERACTIVE`取得にも`UsageEvents`経由でこの権限が要る）。
  `SCREEN_INTERACTIVE`はAPI 28以降のみのため、`minSdk`を24から28に引き上げた。

- **`nativeOnResume`（JNI）**: `MainActivity.onResume()`から`src/platform/android_bg.rs`の
  `Java_com_sleeptracker_app_MainActivity_nativeOnResume`を直接呼び出し、Rust側の本同期
  （Drive/Sheetのpull+merge+push、ホーム画面の同期アイコン回転）をキックする。
  コールドスタート・タスク切り替えからの復帰・画面ロック解除からの復帰・権限設定画面からの
  帰還など「人間が操作を再開した」あらゆる経路でAndroidは必ず`onResume()`を呼ぶため、
  ここ一箇所にフックすれば全経路をカバーできる（Rust側の5分ごとの定期タイマーだけでは、
  画面OFFからの復帰時などにアイコンが反応しないという問題があった）。

Tauri版（`legacy/src_tauri/src-tauri/gen/android/.../MainActivity.kt` および
`DriveSignalWorker.kt`）のWorkManager登録ロジックを参考にしたが、上記の理由で
定期実行の仕組みそのものは踏襲しておらず、最終的にはDEVICE_ON送信自体を廃止した。
またSlint版はWebViewを使わないため、
Tauri版にあった以下の要素も移植していない：

- `AppBridge` JSインターフェース（`setTab`/`notifyReady`）
- ハードウェア戻るボタンのカスタム処理
- 黒画面/WebViewフリーズ対策（`recreate()` ワークアラウンド） — WebView自体を使わないため
  この不具合クラスが構造的に発生しない

## 依存関係

- 依存する（import する）フォルダ：なし（Rust側の `cargo ndk` ビルド出力である
  `jniLibs/*.so` を実行時に読み込むが、ソース上のimportではない）
- 依存される（import される）フォルダ：なし
