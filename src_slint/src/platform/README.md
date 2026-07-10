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
| `android/` | Android専用機能一式（エントリポイント・フォアグラウンド同期・復元・使用状況ブリッジ、Androidのみ）。詳細は`android/README.md`参照 |

## 依存関係

- 依存する（import する）フォルダ：`../core`, `../ui`（`monitor.rs`/`android/`から`home`/`sync_status`/`events`を利用）
- 依存される（import される）フォルダ：`../`（lib.rs）
