# detail

Layer 2 日別詳細機能モジュール。選択した日の睡眠セッション詳細・編集モーダルを提供する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| DayDetail.tsx | 1日の睡眠セッション詳細・追加・削除モーダル |
| index.ts | 公開 API の re-export エントリポイント |

## 依存関係

- 依存する（import する）フォルダ：`../core`（Session, formatDuration, callCount）, `@tauri-apps/api/core`
- 依存される（import される）フォルダ：`../App.tsx`

## 公開 API

```typescript
export { DayDetail }
```
