# prediction

Layer 2 睡眠予測機能モジュール。入眠時刻から予測睡眠時間・起床時刻を表示する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| PredictionCard.tsx | ホーム画面のインライン予測カード |
| PredictionTab.tsx | 設定タブ内の詳細予測パネル |
| index.ts | 公開 API の re-export エントリポイント |

## 依存関係

- 依存する（import する）フォルダ：`../core`（Session, formatDuration, callCount）, `../ui`（TimePicker）, `@tauri-apps/api/core`
- 依存される（import される）フォルダ：`../App.tsx`

## 公開 API

```typescript
export { PredictionCard, PredictionTab }
```
