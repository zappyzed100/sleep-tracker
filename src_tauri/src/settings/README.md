# settings

Layer 2 設定機能モジュール。アプリの全設定画面を提供する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| Settings.tsx | 起動設定・睡眠判定時間・クラウド連携・データ管理の設定画面 |
| index.ts | 公開 API の re-export エントリポイント |

## 依存関係

- 依存する（import する）フォルダ：`../core`（Session, callCount）, `../ui`（TimePicker）, `@tauri-apps/api/core`, `@tauri-apps/plugin-dialog`
- 依存される（import される）フォルダ：`../App.tsx`

## 公開 API

```typescript
export { Settings }
```
