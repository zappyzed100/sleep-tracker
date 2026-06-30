// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// WeeklyChart.tsx — 週間睡眠チャート（棒グラフ＋折れ線グラフ）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 週 7 日分の睡眠時間を棒グラフ、入眠・起床時刻を折れ線グラフで表示する。
//        タップ／クリックで DayDetail を開く。タッチスワイプ（親）との競合を防ぐ
//        透明オーバーレイ方式でイベントを処理する。
//        Chart.js は動的インポートで遅延ロード（初期バンドルから除外）。
//
// 依存 : core（DaySummary, formatDuration）, chart.js（動的）
// 公開 : default export WeeklyChart
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import { useEffect, useRef, useState } from "react";
import { DaySummary, formatDuration, callCount } from "../core";

const TAG = "[chart]";

const DAYS_JA = ["月", "火", "水", "木", "金", "土", "日"];
const CAT = { CRUST: "#313244", GREEN: "#a6e3a1", YELLOW: "#f9e2af", TEXT: "#cdd6f4", SUBTEXT: "#a6adc8" };

const BAR_ACTIVE = "#89b4fa";
const BAR_NORMAL = "rgba(137,180,250,0.45)";

// Chart.js は1プロセス内で1回だけ register すれば良い
let chartJsRegistered = false;

interface Props {
  week: DaySummary[];
  onDayClick: (date: string) => void;
  activeIndex?: number;
}

export default function WeeklyChart({ week, onDayClick, activeIndex }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const chartRef = useRef<any>(null);
  const [chartReady, setChartReady] = useState(false);
  const touchStartXRef = useRef<number | null>(null);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  function barColors(len: number, active: number | undefined) {
    return Array.from({ length: len }, (_, i) =>
      i === active ? BAR_ACTIVE : BAR_NORMAL
    );
  }

  useEffect(() => {
    if (!canvasRef.current) return;
    if (chartRef.current) { chartRef.current.destroy(); chartRef.current = null; }

    const n = callCount(TAG, "render");
    const t0 = performance.now();
    let isMounted = true;

    import("chart.js").then(({
      Chart,
      BarElement, LineElement, PointElement,
      BarController, LineController,
      CategoryScale, LinearScale,
      Tooltip, Legend,
    }) => {
      if (!isMounted || !canvasRef.current) return;

      if (!chartJsRegistered) {
        Chart.register(
          BarElement, LineElement, PointElement,
          BarController, LineController,
          CategoryScale, LinearScale,
          Tooltip, Legend,
        );
        chartJsRegistered = true;
      }

      const labels = week.map((d, i) => {
        const [, m, day] = d.date.split("-");
        return `${DAYS_JA[i]}\n${parseInt(m)}/${parseInt(day)}`;
      });

      const durations = week.map((d) => d.totalHours || null);
      const bedtimes = week.map((d) => d.bedtimeH);
      const waketimes = week.map((d) => d.waketimeH);

      const allY2 = [...bedtimes, ...waketimes].filter((v) => v !== null) as number[];
      const y2Min = allY2.length > 0 ? Math.floor(Math.min(...allY2)) - 1 : 20;
      const y2Max = allY2.length > 0 ? Math.ceil(Math.max(...allY2)) + 1 : 32;
      const y2Step = Math.max(1, Math.round((y2Max - y2Min) / 6));

      const durationPlugin = {
        id: "durationLabels",
        afterDatasetDraw(chart: any) {
          const { ctx } = chart;
          const meta = chart.getDatasetMeta(0);
          meta.data.forEach((bar: any, i: number) => {
            const val = durations[i];
            if (!val) return;
            const barHeight = bar.base - bar.y;
            ctx.save();
            ctx.font = "bold 14px sans-serif";
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            if (barHeight > 22) {
              ctx.fillStyle = CAT.CRUST;
              ctx.fillText(formatDuration(val), bar.x, bar.y + barHeight / 2);
            } else {
              ctx.fillStyle = CAT.TEXT;
              ctx.fillText(formatDuration(val), bar.x, bar.y - 10);
            }
            ctx.restore();
          });
        },
      };

      chartRef.current = new Chart(canvasRef.current, {
        type: "bar",
        plugins: [durationPlugin],
        data: {
          labels,
          datasets: [
            {
              label: "睡眠時間",
              data: durations,
              backgroundColor: barColors(week.length, activeIndex),
              borderColor: "#89b4fa",
              borderWidth: 1,
              yAxisID: "y",
              order: 2,
            },
            {
              label: "入眠",
              data: bedtimes,
              type: "line",
              borderColor: CAT.YELLOW,
              backgroundColor: CAT.YELLOW,
              pointStyle: "circle",
              pointRadius: 5,
              tension: 0.3,
              yAxisID: "y2",
              order: 1,
            },
            {
              label: "起床",
              data: waketimes,
              type: "line",
              borderColor: CAT.GREEN,
              backgroundColor: CAT.GREEN,
              pointStyle: "rect",
              pointRadius: 5,
              tension: 0.3,
              yAxisID: "y2",
              order: 1,
            },
          ],
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          animation: false,
          hover: { mode: undefined },
          events: [],
          plugins: {
            legend: {
              labels: { color: CAT.TEXT, font: { size: 14 } },
            },
            tooltip: {
              callbacks: {
                label(ctx: any) {
                  const y = ctx.parsed.y;
                  if (y == null) return "";
                  if (ctx.datasetIndex === 0) return ` ${formatDuration(y)}`;
                  const h = Math.floor(y % 24);
                  const m = Math.round((y % 1) * 60);
                  return ` ${h}:${String(m).padStart(2, "0")}`;
                },
              },
            },
          },
          scales: {
            x: {
              ticks: { color: CAT.TEXT, font: { size: 14 } },
              grid: { color: "rgba(255,255,255,0.05)" },
            },
            y: {
              position: "left",
              min: 0,
              max: Math.ceil(Math.max(...durations.map((d) => d ?? 0), 6) + 1),
              ticks: {
                color: CAT.TEXT,
                font: { size: 13 },
                callback: (v: any) => `${v}h`,
              },
              grid: { color: "rgba(255,255,255,0.08)" },
            },
            y2: {
              position: "right",
              min: y2Min,
              max: y2Max,
              ticks: {
                color: CAT.SUBTEXT,
                font: { size: 13 },
                callback: (v: any) => {
                  const h = Math.floor((v as number) % 24);
                  return `${h}:00`;
                },
                stepSize: y2Step,
              },
              grid: { drawOnChartArea: false },
            },
          },
        },
      });

      setChartReady(true);

      const ms = Math.round(performance.now() - t0);
      if (ms > 100) {
        console.warn(TAG, `render #${n}: 7 days  (+${ms}ms)`);
      } else {
        console.log(TAG, `render #${n}: 7 days`);
      }
    });

    return () => {
      isMounted = false;
      if (chartRef.current) { chartRef.current.destroy(); chartRef.current = null; }
    };
  }, [week]);

  // activeIndex 変化時はバー色だけ更新（再描画なし）
  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    chart.data.datasets[0].backgroundColor = barColors(week.length, activeIndex);
    chart.update("none");
  }, [activeIndex, chartReady]);

  function hitColumn(clientX: number, rect: DOMRect): number | null {
    const chart = chartRef.current;
    if (!chart) return null;
    const x = clientX - rect.left;
    const raw = chart.scales["x"].getValueForPixel(x);
    if (raw == null) return null;
    const idx = Math.round(raw);
    return idx >= 0 && idx < week.length ? idx : null;
  }

  function cancelLongPress() {
    if (longPressTimer.current) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  }

  return (
    <div style={{ position: "relative", width: "100%", height: "100%" }}>
      {/* Chart.js ロード中のスケルトン */}
      {!chartReady && (
        <div style={{
          position: "absolute", inset: 0,
          display: "flex", alignItems: "center", justifyContent: "center",
          color: CAT.SUBTEXT, fontSize: "14px", pointerEvents: "none",
        }}>
          グラフ読み込み中…
        </div>
      )}
      <canvas
        ref={canvasRef}
        style={{ width: "100%", height: "100%", display: "block", opacity: chartReady ? 1 : 0 }}
      />
      {/* 透明オーバーレイ：タッチ／クリックをChart.jsから切り離す */}
      <div
        style={{ position: "absolute", inset: 0, cursor: "pointer", WebkitTapHighlightColor: "transparent" }}
        onClick={(e) => {
          const idx = hitColumn(e.clientX, e.currentTarget.getBoundingClientRect());
          if (idx != null) onDayClick(week[idx].date);
        }}
        onTouchStart={(e) => {
          const touch = e.touches[0];
          touchStartXRef.current = touch.clientX;
          const rect = e.currentTarget.getBoundingClientRect();
          const touchX = touch.clientX;
          longPressTimer.current = setTimeout(() => {
            longPressTimer.current = null;
            touchStartXRef.current = null;
            const idx = hitColumn(touchX, rect);
            if (idx != null) onDayClick(week[idx].date);
          }, 450);
        }}
        onTouchMove={(e) => {
          if (touchStartXRef.current == null) return;
          const dx = Math.abs(e.touches[0].clientX - touchStartXRef.current);
          if (dx > 10) cancelLongPress();
        }}
        onTouchEnd={(e) => {
          e.preventDefault();
          if (!longPressTimer.current) {
            touchStartXRef.current = null;
            return;
          }
          cancelLongPress();
          const startX = touchStartXRef.current;
          touchStartXRef.current = null;
          if (startX == null) return;
          const endX = e.changedTouches[0].clientX;
          if (Math.abs(endX - startX) >= 60) return;
          const idx = hitColumn(endX, e.currentTarget.getBoundingClientRect());
          if (idx != null) onDayClick(week[idx].date);
        }}
      />
    </div>
  );
}
