<!-- PLAN.md — Sleep Tracker の全体計画・アーキテクチャ・技術選定理由の正本 -->
# PLAN.md — Sleep Tracker 全体計画

## 目的

PC のキーボード・マウス操作の有無からアイドル時間を自動検知し、「PC を長時間触っていない＝
睡眠中」という仮定で睡眠時間を記録・可視化・予測する自分専用アプリ。Windows 版（常駐監視）と
Android 版（ビューアー）が Google スプレッドシート／Drive 経由でデータを同期する。
入力の手間をゼロにすること（手動記録に頼らない）を最優先とする。

## アーキテクチャ

現行実装は `src_slint/`（Rust + Slint、PC・Android 共通コードベース）。
C++ → Python → React/Tauri と実装を変遷しており、過去の実装は `legacy/` に参照用として残るのみで
メンテナンス対象外（詳細は [README.md](README.md) 「仕組み」参照）。

```text
src_slint/src/
  lib.rs / main.rs   Layer 2  全体の配線（core/platform/ui すべてに依存してよい）
  ui/                 Layer 1  画面ロジック（Slint UIへのデータバインディング。coreに依存可、platformには依存しない）
  platform/           Layer 1  OS固有機能（Windows: 監視スレッド・トレイ・スタートアップ／Android: エントリポイント・使用状況ブリッジ）
  core/               Layer 0  ビジネスロジック（config/events/cloud/prediction）。他レイヤーに依存しない
  ui/main.slint                全UI定義（ホーム・設定・日別詳細モーダル）

worker/appsscript.gs    クラウド中継（スプレッドシート記録・Driveバックアップ・モバイルイベント受信）
```

依存方向は上記の一方向のみ（`core` → `platform`/`ui` → `lib.rs`）。詳細・禁止依存は
[CLAUDE.md](CLAUDE.md) §5 とレイヤーごとの README（`src_slint/src/*/README.md`）が正本。

## 技術選定理由

- **Rust + Slint**: UI を含めて単一言語（Rust）で完結させ、PC・Android を同一ロジックで実装する。
  以前の Tauri（Rust + React + WebView）版は Android の WebView フリーズ・黒画面系の不具合が
  頻発しており、WebView を使わない Slint への移行でこれが構造的に解消される
  （詳細は [src_slint/README.md](src_slint/README.md) 「背景」）。
- **Google スプレッドシート + Apps Script + Drive**: 自分専用アプリのためサーバーを持たず、
  Google アカウントだけで PC/Android 間の同期とバックアップを完結させる。
- **PCアイドル検知による自動記録**: 手動入力に頼ると記録が続かないため、キーボード・マウス操作の
  有無だけで睡眠を推定する。GPS（外出）とタブレットの画面ON/OFF実区間を「起きていた証拠」として
  除外に使い、誤検知を減らす（詳細は README「外出中・タブレット利用中の除外」）。
- **Random Forest Regressor（睡眠時間予測）**: データ 10 件未満は同時刻帯の過去平均、10 件以上は
  入眠時刻・曜日・連続覚醒時間などを特徴量とした Random Forest に切り替える。単純な統計だと
  個人の生活パターン（曜日差・連続覚醒の影響）を拾えないため。
- **cargo-ndk + Gradle（Android）**: Rust の cdylib を Kotlin 側（`UsageStatsManager` 連携）と
  組み合わせて配布用 apk をビルドする。純 Rust 反復開発用に `cargo apk` 経路も別途用意している。

## 運用

- リリースは GitHub Releases への手動アップロード（`SleepTracker-Windows.exe` /
  `SleepTracker-Android.apk`）。CI による自動ビルドは未整備。
- Windows は常駐監視スレッドが毎時判定するため確実に日次バックアップを取得できるが、Android は
  常駐スレッドを持てず、アプリを開いたタイミングでのみ判定される（README「ローカル自動バックアップ」）。
- ローカルデータ（`sleep_events.txt` 等）は git 管理外。設定（`config.json`、Apps Script
  URL・シークレット）も git 管理外。

## ロードマップ

1. **記録・可視化・予測の基盤**（完了）: PC自動検知、週別グラフ、Drive同期、Random Forest予測。
2. **誤検知の除去**（完了）: 外出除外（GPS）、タブレット利用除外（画面ON実区間ベース）。
3. **予測精度の向上**（進行中）: 特徴量拡張、カイ二乗ペリオドグラムによる周期検出。
4. **配布の整備**（未着手）: Windows インストーラー（`cargo-wix` のエラーで保留中）、
   Android リリース署名（現状 debug 署名の apk を配布）。
5. Slint 移行の残課題を解消したら、`legacy/` 以下（旧 C++/Python/Tauri 実装）の扱いを再検討する。

## タスク（機械可読）

書式:
- `- [ ] タイトル` … 未完了。行末に `` `状態タグ` `` が無ければ `backlog` 扱い
- `- [x] タイトル` … 完了。行末にタグが無ければ `done` 扱い
- 状態を明示したい時だけ行末にタグを付ける: `` `next` `` / `` `in_progress` `` /
  `` `blocked` `` / `` `cancelled` ``（`done`/`backlog` はチェック状態で表せるため省略可）

- [x] 睡眠時間予測の特徴量を拡張し予測方法表示を削除
- [x] カイ二乗ペリオドグラムによる睡眠周期の検出を追加
- [x] SCREEN_ON区間の無視しきい値を設定可能にし、デフォルトを5分に変更
- [x] アプリ単位のフォアグラウンド検出(APP_USAGE)を画面ON区間検出(SCREEN_ON)に置き換え
- [ ] Windowsインストーラー（MSI/NSIS）を整備する（cargo-wixのエラー原因を調査） `blocked`
- [ ] Androidのリリース署名（keystore）を整備し、debug署名から切り替える `backlog`
- [ ] GitHub Releasesへのビルド・アップロードをCIで自動化する `backlog`
