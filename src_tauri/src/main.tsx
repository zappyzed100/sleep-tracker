// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// main.tsx — React アプリケーションのマウントエントリポイント
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : DOM の #root 要素に React アプリをマウントする。
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// Chart.js をバックグラウンドでプリロード（起動高速化）
import("chart.js").catch(() => { /* プリロード失敗は無視 */ });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
