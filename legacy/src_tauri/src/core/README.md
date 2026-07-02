# core

Layer 0 基盤モジュール。型定義・ユーティリティ・ログカウンターを提供する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| types.ts | Session・DaySummary 型定義 |
| utils.ts | 日付操作・週データ構築ユーティリティ |
| logger.ts | 呼び出し回数カウンター（callCount / dumpCounts） |
| index.ts | 公開 API の re-export エントリポイント |

## 依存関係

- 依存する（import する）フォルダ：なし（Layer 0 は他のレイヤーを import しない）
- 依存される（import される）フォルダ：`ui/`, `chart/`, `detail/`, `prediction/`, `settings/`, `App.tsx`

## 公開 API

```typescript
// 型
export type { Session, DaySummary }

// utils
export { parseLocalDate, weekStart, addDays, isoDate, toNightHour, buildWeek, formatDuration }

// logger
export { callCount, dumpCounts }
```
