# chart

Layer 2 チャート機能モジュール。週間睡眠チャートと統計カードを提供する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| WeeklyChart.tsx | 週間睡眠チャート（棒グラフ＋折れ線グラフ）|
| StatsCard.tsx | 期間別睡眠統計カード |
| index.ts | 公開 API の re-export エントリポイント |

## 依存関係

- 依存する（import する）フォルダ：`../core`（DaySummary, Session, formatDuration, callCount）, `chart.js`
- 依存される（import される）フォルダ：`../App.tsx`

## 公開 API

```typescript
export { WeeklyChart, StatsCard }
```
