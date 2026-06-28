import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import WeeklyChart from "./WeeklyChart";
import StatsCard from "./StatsCard";
import PredictionCard from "./PredictionCard";
import CalendarPicker from "./CalendarPicker";
import DayDetail from "./DayDetail";
import Settings from "./Settings";
import { Session } from "./types";
import { buildWeek, weekStart, addDays } from "./utils";
import "./App.css";

const DAYS_JA = ["月", "火", "水", "木", "金", "土", "日"];

const USE_DUMMY = false;

function makeDummy(): Session[] {
  const result: Session[] = [];
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const fmt = (d: Date) =>
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:00`;
  for (let i = 30; i >= 1; i--) {
    const bedH = 22 + Math.random() * 2;
    const durH = 6.5 + Math.random() * 2;
    const bed = new Date(now);
    bed.setDate(bed.getDate() - i);
    bed.setHours(Math.floor(bedH), Math.round((bedH % 1) * 60), 0, 0);
    const wake = new Date(bed.getTime() + durH * 3600_000);
    result.push({ start: fmt(bed), end: fmt(wake), duration: durH, type: "IDLE" });
  }
  return result;
}

function fmtDateRange(base: Date): string {
  const s = weekStart(base);
  const e = addDays(s, 6);
  const fmt = (d: Date) =>
    `${d.getFullYear()}/${String(d.getMonth() + 1).padStart(2, "0")}/${String(d.getDate()).padStart(2, "0")} (${DAYS_JA[d.getDay() === 0 ? 6 : d.getDay() - 1]})`;
  return `${fmt(s)} 〜 ${fmt(e)}`;
}

type Tab = "home" | "settings";
type MonitorStatus = "active" | "paused" | "inactive";

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [sessions, setSessions] = useState<Session[]>([]);
  const [weekBase, setWeekBase] = useState(new Date());
  const [error, setError] = useState<string | null>(null);
  const [selectedDay, setSelectedDay] = useState<string | null>(null);
  const [showCal, setShowCal] = useState(false);
  const calBtnRef = useRef<HTMLButtonElement>(null);
  const [monitorStatus, setMonitorStatus] = useState<MonitorStatus>("inactive");

  const loadSessions = useCallback(async () => {
    if (USE_DUMMY) { setSessions(makeDummy()); return; }
    try {
      const data = await invoke<Session[]>("get_sessions");
      setSessions(data);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => { loadSessions(); }, [loadSessions]);

  const pollMonitor = useCallback(async () => {
    if (USE_DUMMY) return;
    try {
      const s = await invoke<string>("get_monitor_status");
      setMonitorStatus(s as MonitorStatus);
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    pollMonitor();
    const id = setInterval(pollMonitor, 30_000);
    return () => clearInterval(id);
  }, [pollMonitor]);

  async function toggleMonitorPause() {
    const shouldPause = monitorStatus === "active";
    try {
      await invoke("set_monitor_paused", { paused: shouldPause });
      setMonitorStatus(shouldPause ? "paused" : "active");
    } catch (e) {
      console.error(e);
    }
  }

  useEffect(() => {
    const handler = (e: WheelEvent) => {
      if (tab !== "home") return;
      setWeekBase((prev) => addDays(prev, e.deltaY > 0 ? 7 : -7));
    };
    window.addEventListener("wheel", handler, { passive: true });
    return () => window.removeEventListener("wheel", handler);
  }, [tab]);

  const week = buildWeek(sessions, weekBase);

  return (
    <div className="app">
      <div className="topbar">
        <span className="app-title">睡眠トラッカー</span>
        <div className="tabs">
          <button className={tab === "home" ? "tab active" : "tab"} onClick={() => setTab("home")}>ホーム</button>
          <button className={tab === "settings" ? "tab active" : "tab"} onClick={() => setTab("settings")}>設定</button>
        </div>
      </div>

      {error && <div className="error-banner">{error}</div>}

      {tab === "home" && (
        <>
          <div className="monitor-bar">
            <span className={`monitor-dot monitor-dot-${monitorStatus}`} />
            <span className="monitor-label">
              {monitorStatus === "active" && "検知中"}
              {monitorStatus === "paused" && "検知中断中"}
              {monitorStatus === "inactive" && "停止中"}
            </span>
            {(monitorStatus === "active" || monitorStatus === "paused") && (
              <button className="monitor-toggle-btn" onClick={toggleMonitorPause}>
                {monitorStatus === "active" ? "中断する" : "再開する"}
              </button>
            )}
          </div>

          <PredictionCard sessions={sessions} />
          <StatsCard sessions={sessions} />

          <div className="nav-bar">
            <button onClick={() => setWeekBase((p) => addDays(p, -7))}>◀ 前の週</button>
            <span className="week-label">{fmtDateRange(weekBase)}</span>
            <div className="nav-cal-wrap">
              <button
                ref={calBtnRef}
                className="nav-cal-btn"
                onClick={() => setShowCal((v) => !v)}
                title="カレンダーで移動"
              >
                📅
              </button>
              {showCal && (
                <CalendarPicker
                  current={weekBase}
                  onSelect={(d) => setWeekBase(d)}
                  onClose={() => setShowCal(false)}
                  anchorRef={calBtnRef}
                />
              )}
            </div>
            <button onClick={() => setWeekBase(new Date())}>今週</button>
            <button onClick={() => setWeekBase((p) => addDays(p, 7))}>次の週 ▶</button>
          </div>

          <div className="chart-area">
            <WeeklyChart week={week} onDayClick={setSelectedDay} />
          </div>

          {selectedDay && (
            <DayDetail
              date={selectedDay}
              sessions={sessions}
              onClose={() => setSelectedDay(null)}
              onRefresh={loadSessions}
            />
          )}
        </>
      )}

      {tab === "settings" && <Settings sessions={sessions} />}
    </div>
  );
}
