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
  const [isMobile, setIsMobile] = useState(false);
  const [syncing, setSyncing] = useState(false);

  // Detect platform on mount
  useEffect(() => {
    invoke<boolean>("is_mobile").then(setIsMobile).catch(() => {});
  }, []);

  const loadSessions = useCallback(async () => {
    try {
      const data = await invoke<Session[]>("get_sessions");
      setSessions(data);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => { loadSessions(); }, [loadSessions]);

  // Android: fetch from Drive on mount, then every 5 min
  useEffect(() => {
    if (!isMobile) return;
    const doFetch = async () => {
      try {
        const data = await invoke<Session[]>("fetch_from_cloud");
        setSessions(data);
        setError(null);
      } catch {
        // If fetch fails (no connection, unconfigured), fall back to local cache
        loadSessions();
      }
    };
    doFetch();
    const id = setInterval(doFetch, 5 * 60 * 1000);
    return () => clearInterval(id);
  }, [isMobile, loadSessions]);

  // Android: send SCREEN_ON immediately on load, then every 5 min
  useEffect(() => {
    if (!isMobile) return;
    const send = () => invoke("send_screen_on").catch(() => {});
    send();
    const id = setInterval(send, 5 * 60 * 1000);
    return () => clearInterval(id);
  }, [isMobile]);

  // Desktop: poll monitor status
  const pollMonitor = useCallback(async () => {
    try {
      const s = await invoke<string>("get_monitor_status");
      setMonitorStatus(s as MonitorStatus);
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    if (isMobile) return;
    pollMonitor();
    const id = setInterval(pollMonitor, 30_000);
    return () => clearInterval(id);
  }, [pollMonitor, isMobile]);

  async function toggleMonitorPause() {
    const shouldPause = monitorStatus === "active";
    try {
      await invoke("set_monitor_paused", { paused: shouldPause });
      setMonitorStatus(shouldPause ? "paused" : "active");
    } catch (e) {
      setError(`モニター操作失敗: ${e}`);
    }
  }

  // Android: manual sync button — fetch from Drive immediately
  async function handleAndroidSync() {
    setSyncing(true);
    setError(null);
    try {
      const data = await invoke<Session[]>("fetch_from_cloud");
      setSessions(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncing(false);
    }
  }

  useEffect(() => {
    const handler = (e: WheelEvent) => {
      if (tab !== "home") return;
      if (selectedDay !== null) return;
      setWeekBase((prev) => addDays(prev, e.deltaY > 0 ? 7 : -7));
    };
    window.addEventListener("wheel", handler, { passive: true });
    return () => window.removeEventListener("wheel", handler);
  }, [tab, selectedDay]);

  const week = buildWeek(sessions, weekBase);

  return (
    <div className="app">
      <div className="topbar">
        <span className="app-title">睡眠トラッカー</span>
        <div className="tabs">
          <button className={tab === "home" ? "tab active" : "tab"} onClick={() => setTab("home")}>ホーム</button>
          <button className={tab === "settings" ? "tab active" : "tab"} onClick={() => setTab("settings")}>設定</button>
        </div>

        {/* Desktop: monitor status */}
        {!isMobile && (
          <div className="monitor-inline">
            <span className={`monitor-dot monitor-dot-${monitorStatus}`} />
            <span className="monitor-label">
              {monitorStatus === "active" && "検知中"}
              {monitorStatus === "paused" && "検知中断中"}
              {monitorStatus === "inactive" && "停止中"}
            </span>
            <button className="monitor-toggle-btn" onClick={toggleMonitorPause}>
              {monitorStatus === "active" ? "中断する" : "再開する"}
            </button>
          </div>
        )}

        {/* Android: sync button */}
        {isMobile && (
          <button
            className="monitor-toggle-btn"
            onClick={handleAndroidSync}
            disabled={syncing}
          >
            {syncing ? "同期中..." : "同期"}
          </button>
        )}
      </div>

      {error && <div className="error-banner">{error}</div>}

      {tab === "home" && (
        <>
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

      {tab === "settings" && (
        <Settings sessions={sessions} onRefresh={loadSessions} isMobile={isMobile} />
      )}
    </div>
  );
}
