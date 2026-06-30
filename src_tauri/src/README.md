# src — フロントエンドルート

Tauri + React フロントエンドのソースルート。レイヤー構造に従いフォルダを分割している。

## フォルダ構成

| フォルダ / ファイル | レイヤー | 役割 |
|--------------------|---------|------|
| `core/` | Layer 0 | 型定義・ユーティリティ・ログカウンター |
| `ui/` | Layer 1 | 汎用 UI コンポーネント（TimePicker, CalendarPicker）|
| `chart/` | Layer 2 | 週間チャート・統計カード |
| `detail/` | Layer 2 | 日別詳細・セッション編集モーダル |
| `prediction/` | Layer 2 | 睡眠予測カード・タブ |
| `settings/` | Layer 2 | 設定画面 |
| `App.tsx` | Layer 3 | アプリルートコンポーネント（全体の組み合わせ）|
| `App.css` | — | グローバルスタイル |
| `main.tsx` | — | React マウントエントリポイント |
| `vite-env.d.ts` | — | Vite 環境型定義 |

## 依存レイヤー規則

```
Layer 3 (App.tsx) → Layer 2 (features) → Layer 1 (ui) → Layer 0 (core)
```

上位レイヤーから下位レイヤーへの一方向のみ。逆方向・循環は禁止。
