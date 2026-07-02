# legacy

Tauri 版（`src_tauri/`）に置き換えられた旧実装。現在はメンテナンス対象外。

## 含まれるフォルダ・ファイル

| フォルダ / ファイル | 役割 |
|---------------------|------|
| src_cpp/ | 旧・C++ バックグラウンド監視サービス（Windows API でアイドル・電源イベントを監視） |
| src_python/ | 旧・Python 解析/予測/GUI ツール一式（`src_cpp` の代替監視スクリプトを含む） |
| pyproject.toml, uv.lock, .python-version | `src_python/` 用の Python プロジェクト設定 |
| monitor_err.log, monitor_err.txt | 旧監視プロセスのエラーログ（空ファイル） |

## 依存関係

- 依存する（import する）フォルダ：なし
- 依存される（import される）フォルダ：なし（現行の `src_tauri/` からは一切参照されていない）

## 補足

新規開発・修正はすべて `src_tauri/` で行う。このフォルダは過去の実装を参照用に残しているのみで、動作保証はしない。
