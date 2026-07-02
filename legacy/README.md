# legacy

過去に置き換えられた実装のスナップショット。現在はメンテナンス対象外。

## 含まれるフォルダ・ファイル

| フォルダ / ファイル | 役割 |
|---------------------|------|
| src_cpp/ | 旧・C++ バックグラウンド監視サービス（Windows API でアイドル・電源イベントを監視） |
| src_python/ | 旧・Python 解析/予測/GUI ツール一式（`src_cpp` の代替監視スクリプトを含む） |
| pyproject.toml, uv.lock, .python-version | `src_python/` 用の Python プロジェクト設定 |
| monitor_err.log, monitor_err.txt | 旧監視プロセスのエラーログ（空ファイル） |
| src_tauri/ | Flutter + Rust 移行前の Tauri + Rust 版のソースコード スナップショット（ビルド成果物・node_modules・署名鍵は除外） |
| worker/appsscript.gs | Tauri 版が使用していた Google Apps Script（モバイル受信・Drive バックアップ） |

## 依存関係

- 依存する（import する）フォルダ：なし
- 依存される（import される）フォルダ：なし（現行の実装からは一切参照されていない）

## 補足

新規開発・修正は現行の実装（Flutter + Rust 版）で行う。このフォルダは過去の実装を参照用に残しているのみで、動作保証はしない。

`legacy/src_tauri/` にはビルド成果物（`node_modules/`, `src-tauri/target/`, `gen/android` のビルド出力）と
Android 署名鍵（`*.keystore`）を含めていない。ビルドを再現する場合は `pnpm install` 等が別途必要。
