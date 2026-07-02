// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// App.tsx — アプリケーションルートコンポーネント
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : タブ切り替え・週ナビゲーション・セッション取得・クラウド同期など
//        アプリ全体の状態管理と UI 組み合わせを行う Layer 3 コンポーネント。
//
// 依存 : core, chart, detail, prediction, settings, ui
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import { useState, useEffect, useCallback, useRef, useMemo, startTransition } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { WeeklyChart, StatsCard } from "./chart";
import { PredictionCard } from "./prediction";
import { CalendarPicker } from "./ui";
import { DayDetail } from "./detail";
import { Settings } from "./settings";
import { Session, buildWeek, weekStart, addDays, callCount } from "./core";
import "./App.css";

const TAG = "[app]";

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
  // AppBridge is injected synchronously before JS runs on Android — check at init time.
  const [isMobile, setIsMobile] = useState(() => typeof (window as any).AppBridge !== 'undefined');
  // バックグラウンド復帰時の黒画面対策: ローディングオーバーレイ
  const [resuming, setResuming] = useState(false);
  const [screenOnEnabled, setScreenOnEnabled] = useState(true);
  const [appVersion, setAppVersion] = useState("");
  const touchStartX = useRef<number | null>(null);

  // Detect platform and load screen_on_enabled config on mount
  useEffect(() => {
    // React has mounted and click handlers are attached — dismiss native startup overlay
    (window as any).AppBridge?.notifyReady?.();

    invoke<boolean>("is_mobile").then(mobile => {
      const platform = mobile ? "Android" : "PC";
      console.log(TAG, `mount: platform=${platform}`);
      setIsMobile(mobile);
      if (!mobile) {
        // PC: fetch settings (idle threshold, target wake time) from Drive on startup
        invoke("fetch_settings_from_cloud")
          .then(() => loadSessions())
          .catch(() => { });
      }
    }).catch(() => { });
    getVersion().then(setAppVersion).catch(() => { });
    invoke<{ screen_on_enabled: boolean | null }>("get_config")
      .then(cfg => setScreenOnEnabled(cfg.screen_on_enabled ?? true))
      .catch(() => { });
  }, []);

  const loadSessions = useCallback(async () => {
    const n = callCount(TAG, "loadSessions");
    const t0 = performance.now();
    try {
      const data = await invoke<Session[]>("get_sessions");
      const ms = Math.round(performance.now() - t0);
      console.log(TAG, `loadSessions #${n}: ${data.length} sessions  (+${ms}ms)`);
      startTransition(() => { setSessions(data); setError(null); });
    } catch (e) {
      console.error(TAG, `ERROR loadSessions #${n}:`, e);
      setError(String(e));
    }
  }, []);

  useEffect(() => { loadSessions(); }, [loadSessions]);

  // Android: full bidirectional sync on mount, every 30 min, and on screen ON / app foreground.
  // sync_mobile merges Drive ↔ local ↔ Sheet, uploads merged result, returns parsed sessions.
  useEffect(() => {
    if (!isMobile) return;

    const effectId = callCount(TAG, "android-effect");
    console.log(TAG, `android-effect #${effectId}: mounted`);

    let lastSync = 0;
    let lastDeviceOn = 0;

    // record_device_on throttled to once per 60s to prevent visibilitychange loop writes
    const doRecordDeviceOn = (reason: string) => {
      const n = callCount(TAG, "record_device_on");
      const now = Date.now();
      if (now - lastDeviceOn < 60_000) {
        console.log(TAG, `record_device_on #${n}: SKIP throttled (reason=${reason}, ${Math.round((now - lastDeviceOn) / 1000)}s ago)`);
        return;
      }
      lastDeviceOn = now;
      console.log(TAG, `record_device_on #${n}: invoking (reason=${reason})`);
      invoke("record_device_on")
        .then(() => console.log(TAG, `record_device_on #${n}: done`))
        .catch(e => console.error(TAG, `ERROR record_device_on #${n}:`, e));
    };

    const doSync = (force = false) => {
      const now = Date.now();
      const elapsedSec = Math.round((now - lastSync) / 1000);
      if (!force && now - lastSync < 60 * 1000) {
        console.log(TAG, `doSync: SKIP throttled (force=${force}, last=${elapsedSec}s ago)`);
        return;
      }
      lastSync = now;
      loadSessions();
      const n = callCount(TAG, "sync_mobile");
      const t0 = performance.now();
      console.log(TAG, `sync_mobile #${n}: started (force=${force}, last=${elapsedSec}s ago)`);
      invoke<Session[]>("sync_mobile")
        .then(data => {
          const ms = Math.round(performance.now() - t0);
          console.log(TAG, `sync_mobile #${n}: ${data.length} sessions  (+${ms}ms)`);
          startTransition(() => { setSessions(data); setError(null); });
        })
        .catch(e => console.error(TAG, `ERROR sync_mobile #${n}:`, e));
    };

    doRecordDeviceOn("startup");
    doSync(true);

    const id = setInterval(() => {
      console.log(TAG, "30min-interval: tick");
      doSync(true);
    }, 30 * 60 * 1000);

    let visibilitySeq = 0;
    const onVisible = () => {
      visibilitySeq++;
      const state = document.visibilityState;
      const n = callCount(TAG, "visibilitychange");
      console.log(TAG, `visibilitychange #${n} (seq=${visibilitySeq}): ${state}`);
      if (state === "visible") {
        // 黒画面対策: まずローディング画面を即時表示し、UIペイント後に重い処理を開始
        setResuming(true);
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            doRecordDeviceOn(`visibilitychange#${visibilitySeq}`);
            doSync();
            // 同期完了後にローディング画面を解除
            setTimeout(() => setResuming(false), 500);
          });
        });
      }
    };
    document.addEventListener("visibilitychange", onVisible);

    console.log(TAG, `android-effect #${effectId}: setup done`);
    return () => {
      console.log(TAG, `android-effect #${effectId}: cleanup`);
      clearInterval(id);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [isMobile, loadSessions]);

  // Android: send SCREEN_ON every 5 min (if enabled) — skip immediate send on mount
  useEffect(() => {
    if (!isMobile || !screenOnEnabled) return;
    const send = () => invoke("send_screen_on").catch(() => { });
    const id = setInterval(send, 15 * 60 * 1000);
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

  // PC: refresh sessions immediately when monitor writes IDLE_RESUME
  useEffect(() => {
    if (isMobile) return;
    const p = listen("sleep-session-recorded", () => {
      console.log(TAG, "sleep-session-recorded: refreshing sessions");
      loadSessions();
    });
    return () => { p.then(fn => fn()); };
  }, [isMobile, loadSessions]);

  async function toggleMonitorPause() {
    const shouldPause = monitorStatus === "active";
    try {
      await invoke("set_monitor_paused", { paused: shouldPause });
      setMonitorStatus(shouldPause ? "paused" : "active");
    } catch (e) {
      setError(`モニター操作失敗: ${e}`);
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

  // Notify Android native bridge of tab changes (for hardware back button)
  useEffect(() => {
    if (isMobile && (window as any).AppBridge) {
      (window as any).AppBridge.setTab(tab);
    }
  }, [tab, isMobile]);

  // Handle Android back button event dispatched from native
  useEffect(() => {
    if (!isMobile) return;
    const handler = () => setTab("home");
    window.addEventListener("__androidBack", handler);
    return () => window.removeEventListener("__androidBack", handler);
  }, [isMobile]);

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

  const week = useMemo(() => buildWeek(sessions, weekBase), [sessions, weekBase]);
  const activeIndex = selectedDay ? week.findIndex((d) => d.date === selectedDay) : undefined;

  return (
    <div className="app">
      {/* バックグラウンド復帰時のローディングオーバーレイ */}
      {resuming && (
        <div style={{
          position: "fixed", inset: 0, zIndex: 9999,
          background: "#1e1e2e", display: "flex",
          alignItems: "center", justifyContent: "center",
          flexDirection: "column", gap: 16,
        }}>
          <div style={{ fontSize: 40 }}>🌙</div>
          <div style={{ color: "#cdd6f4", fontSize: 16 }}>復帰中...</div>
        </div>
      )}
      {/* Android status bar spacer */}
      {isMobile && <div className="safe-area-spacer" />}

      <div className="topbar">
        <span className="app-title">睡眠トラッカー{appVersion && <small className="app-version">v{appVersion}</small>}</span>
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

      {isMobile && <div className="bottom-spacer" />}
    </div>
  );
}