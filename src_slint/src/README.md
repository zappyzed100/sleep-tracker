# src_slint/src

Rustソース一式。`core/`（ビジネスロジック）・`platform/`（OS固有機能）・`ui/`（画面ロジック）の
3レイヤーに分割している。各サブフォルダの詳細はそれぞれの README.md を参照。

## 含まれるファイル・フォルダ

| ファイル / フォルダ | 役割 |
|---------------------|------|
| `lib.rs` | エントリポイント共通ロジック。共有static・パスユーティリティ・全コールバック配線 |
| `main.rs` | デスクトップ版エントリポイント（`lib.rs::run()` を呼ぶだけ） |
| `core/` | ビジネスロジック（config/events/cloud/prediction/utils）。詳細は `core/README.md` |
| `platform/` | OS固有機能（windows/monitor/tray/android/android_bg）。詳細は `platform/README.md` |
| `ui/` | 画面ロジック（home/settings_ui）。詳細は `ui/README.md` |

## レイヤー構造

```
lib.rs / main.rs   Layer 2  全体の配線（core/platform/uiすべてに依存してよい）
ui/                 Layer 1  画面ロジック（coreに依存してよい、platformには依存しない）
platform/           Layer 1  OS固有機能（coreに依存してよい）
core/               Layer 0  ビジネスロジック（他レイヤーに依存しない）
```

## 依存関係

- 依存する（import する）フォルダ：`../ui`（Slint側、`main.slint` をbuild.rs経由でコンパイル）
- 依存される（import される）フォルダ：なし
