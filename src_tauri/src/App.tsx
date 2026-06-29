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
  const [screenOnEnabled, setScreenOnEnabled] = useState(true);
  const touchStartX = useRef<number | null>(null);

  // Detect platform and load screen_on_enabled config on mount
  useEffect(() => {
    invoke<boolean>("is_mobile").then(setIsMobile).catch(() => {});
    invoke<{ screen_on_enabled: boolean | null }>("get_config")
      .then(cfg => setScreenOnEnabled(cfg.screen_on_enabled ?? true))
      .catch(() => {});
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

  // Android: fetch from Drive on mount, then every 30 min
  useEffect(() => {
    if (!isMobile) return;
    const doFetch = async () => {
      try {
        const data = await invoke<Session[]>("fetch_from_cloud");
        setSessions(data);
        setError(null);
      } catch {
        loadSessions();
      }
    };
    doFetch();
    const id = setInterval(doFetch, 30 * 60 * 1000);
    return () => clearInterval(id);
  }, [isMobile, loadSessions]);

  // Android: send SCREEN_ON every 5 min (if enabled)
  useEffect(() => {
    if (!isMobile || !screenOnEnabled) return;
    const send = () => invoke("send_screen_on").catch(() => {});
    send();
    const id = setInterval(send, 5 * 60 * 1000);
    return () => clearInterval(id);
  }, [isMobile, screenOnEnabled]);

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

  // Android: manual sync button
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

  // Wheel: week navigation (desktop)
  useEffect(() => {
    const handler = (e: WheelEvent) => {
      if (tab !== "home") return;
      if (selectedDay !== null) return;
      setWeekBase((prev) => addDays(prev, e.deltaY > 0 ? 7 : -7));
    };
    window.addEventListener("wheel", handler, { passive: true });
    return () => window.removeEventListener("wheel", handler);
  }, [tab, selectedDay]);

  // Touch swipe: week navigation (mobile)
  function handleTouchStart(e: React.TouchEvent) {
    touchStartX.current = e.touches[0].clientX;
  }
  function handleTouchEnd(e: React.TouchEvent) {
    if (touchStartX.current === null) return;
    const dx = e.changedTouches[0].clientX - touchStartX.current;
    touchStartX.current = null;
    if (Math.abs(dx) < 60) return;
    setWeekBase((p) => addDays(p, dx < 0 ? 7 : -7));
  }

  const week = buildWeek(sessions, weekBase);
  const activeIndex = selectedDay ? week.findIndex((d) => d.date === selectedDay) : undefined;

  return (
    <div className="app">
      {/* Android status bar spacer */}
      {isMobile && <div className="safe-area-spacer" />}

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

          <div
            className="chart-area"
            onTouchStart={handleTouchStart}
            onTouchEnd={handleTouchEnd}
          >
            <WeeklyChart week={week} onDayClick={setSelectedDay} activeIndex={activeIndex !== -1 ? activeIndex : undefined} />
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
        <Settings
          sessions={sessions}
          onRefresh={loadSessions}
          isMobile={isMobile}
          onBack={() => setTab("home")}
          onScreenOnEnabledChange={setScreenOnEnabled}
        />
      )}
    </div>
  );
}
