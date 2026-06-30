// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// StatsCard.tsx — 期間別睡眠統計カード
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 先週・先月・1年・全期間の4つの期間タブで、
//        記録日数・平均睡眠時間・最後の睡眠時間を表示する統計カード。
//
// 依存 : core（Session, parseLocalDate, formatDuration）
// 公開 : default export StatsCard
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import { useState } from "react";
import { Session, parseLocalDate, formatDuration } from "../core";

type Period = "week" | "month" | "year" | "all";

const PERIODS: { key: Period; label: string; days: number | null }[] = [
  { key: "week", label: "先週", days: 7 },
  { key: "month", label: "先月", days: 30 },
  { key: "year", label: "1年", days: 365 },
  { key: "all", label: "全期間", days: null },
];

interface Props {
  sessions: Session[];
}

export default function StatsCard({ sessions }: Props) {
  const [period, setPeriod] = useState<Period>("month");

  const cutoff = PERIODS.find((p) => p.key === period)!;
  const now = Date.now();
  const filtered = cutoff.days
    ? sessions.filter((s) => now - parseLocalDate(s.start).getTime() <= cutoff.days! * 86400_000)
    : sessions;

  const uniqueDays = new Set(filtered.map((s) => s.start.slice(0, 10))).size;
  const avg = filtered.length > 0 ? filtered.reduce((sum, s) => sum + s.duration, 0) / filtered.length : null;
  const last = sessions.length > 0 ? sessions[sessions.length - 1].duration : null;

  return (
    <div className="info-strip">
      <div className="strip-label">統計</div>
      <div className="strip-main">
        <div className="period-tabs">
          {PERIODS.map((p) => (
            <button
              key={p.key}
              className={period === p.key ? "period-btn active" : "period-btn"}
              onClick={() => setPeriod(p.key)}
            >
              {p.label}
            </button>
          ))}
        </div>
        <div className="strip-divider" />
        <div className="strip-col">
          <div className="big-value">{uniqueDays}<span className="unit">日</span></div>
          <div className="small-label">記録日数</div>
        </div>
        <div className="strip-divider" />
        <div className="strip-col">
          <div className="big-value blue">{avg != null ? formatDuration(avg) : "—"}</div>
          <div className="small-label">平均睡眠</div>
        </div>
        <div className="strip-divider" />
        <div className="strip-col">
          <div className="big-value">{last != null ? formatDuration(last) : "—"}</div>
          <div className="small-label">最後の睡眠</div>
        </div>
      </div>
    </div>
  );
}
