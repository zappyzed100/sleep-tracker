// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// StatsCard.tsx — 期間別睡眠統計カード
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 先週・先月・1年・全期間の4つの期間タブで、
//        記録日数・平均睡眠時間・最後の睡眠時間を表示する統計カード。
//
// 依存 : core（Session, parseLocalDate, formatDuration）, Tauri invoke/listen
// 公開 : default export StatsCard
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Session, parseLocalDate, formatDuration } from "../core";

type Period = "week" | "month" | "year" | "all";

const PERIODS: { key: Period; label: string; days: number | null }[] = [
  { key: "week", label: "先週", days: 7 },
  { key: "month", label: "先月", days: 30 },
  { key: "year", label: "1年", days: 365 },
  { key: "all", label: "全期間", days: null },
];

function currentNowIso(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:00`;
}

function awakeColor(h: number): string {
  if (h > 16) return "var(--red)";
  if (h > 12) return "var(--yellow)";
  return "var(--green)";
}

interface Props {
  sessions: Session[];
}

export default function StatsCard({ sessions }: Props) {
  const [period, setPeriod] = useState<Period>("month");
  const [clock, setClock] = useState(() => new Date());
  // 起きてからの経過時間の起点（Rust predict_sleep の awake_hours）
  const [awakeBase, setAwakeBase] = useState<{ hours: number; at: number } | null>(null);

  // 現在時刻・起きてから経過時間のライブ更新（10秒ごと）
  useEffect(() => {
    const id = setInterval(() => setClock(new Date()), 10_000);
    return () => clearInterval(id);
  }, []);

  // Rust イベント: トレイに隠れて JS タイマーが間引かれても更新されるように
  useEffect(() => {
    const p = listen("prediction-tick", () => setClock(new Date()));
    return () => { p.then(fn => fn()); };
  }, []);

  // 起きてから経過時間の起点を取得
  useEffect(() => {
    if (sessions.length === 0) { setAwakeBase(null); return; }
    invoke<{ awake_hours: number }>("predict_sleep", { sessions, nowIso: currentNowIso() })
      .then(r => setAwakeBase({ hours: r.awake_hours, at: Date.now() }))
      .catch(() => setAwakeBase(null));
  }, [sessions]);

  const awakeHours = awakeBase != null ? awakeBase.hours + (clock.getTime() - awakeBase.at) / 3_600_000 : null;

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
        <div className="strip-divider" />
        <div className="strip-col">
          <div className="big-value">{`${String(clock.getHours()).padStart(2, "0")}:${String(clock.getMinutes()).padStart(2, "0")}`}</div>
          <div className="small-label">現在時刻</div>
        </div>
        <div className="strip-divider" />
        <div className="strip-col">
          <div className="big-value" style={{ color: awakeHours != null ? awakeColor(awakeHours) : undefined }}>
            {awakeHours != null ? formatDuration(awakeHours) : "—"}
          </div>
          <div className="small-label">起きてから</div>
        </div>
      </div>
    </div>
  );
}
