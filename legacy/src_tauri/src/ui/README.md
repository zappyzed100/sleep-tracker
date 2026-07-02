# ui

Layer 1 汎用 UI コンポーネント。特定の機能に依存しない再利用可能な部品を提供する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| TimePicker.tsx | 時刻（HH:MM）入力用ドロップダウンピッカー |
| CalendarPicker.tsx | 月カレンダー表示の週選択ピッカー |
| index.ts | 公開 API の re-export エントリポイント |

## 依存関係

- 依存する（import する）フォルダ：`../core`（weekStart, addDays, isoDate）
- 依存される（import される）フォルダ：`../prediction`（PredictionCard, PredictionTab）、`../settings`（Settings）、`../App.tsx`

## 公開 API

```typescript
export { TimePicker, CalendarPicker }
```
