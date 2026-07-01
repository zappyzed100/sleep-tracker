// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PredictionCard.tsx — ホーム画面の睡眠予測インラインカード
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 入眠時刻を指定して予測睡眠時間・起床時刻・起きてからの経過時間を表示する。
//        「今すぐ」「最適睡眠」ボタンで入眠時刻を自動設定できる。
//
// 依存 : core（Session, formatDuration, callCount）, ui/TimePicker
// 公開 : default export PredictionCard
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Session, formatDuration, callCount } from "../core";
import { TimePicker } from "../ui";

const TAG = "[prediction]";

interface PredictionResult {
  duration_hours: number;
  method: string;
  awake_hours: number;
}

interface OptimalResult {
  best_bed_time: string;
  expected_wake_time: string;
  duration_hours: number;
}

function currentHHMM(): string {
  const d = new Date();
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function currentNowIso(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:00`;
}

function bedTimeToIso(hhmm: string): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const [h, m] = hhmm.split(":").map(Number);
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(h)}:${p(m)}:00`;
}

function addHoursToHHMM(hhmm: string, hours: number): string {
  const [h, m] = hhmm.split(":").map(Number);
  const totalMins = h * 60 + m + Math.round(hours * 60);
  return `${String(Math.floor(totalMins / 60) % 24).padStart(2, "0")}:${String(totalMins % 60).padStart(2, "0")}`;
}

function awakeColor(h: number): string {
  if (h > 16) return "var(--red)";
  if (h > 12) return "var(--yellow)";
  return "var(--green)";
}

interface Props {
  sessions: Session[];
}

export default function PredictionCard({ sessions }: Props) {
  const [bedTime, setBedTime] = useState(currentHHMM);
  const [result, setResult] = useState<PredictionResult | null>(null);
  const [loadingOptimal, setLoadingOptimal] = useState(false);
  const [tick, setTick] = useState(0);

  // Refresh every minute so "起きてから" stays current without user interaction.
  useEffect(() => {
    const id = setInterval(() => setTick(t => t + 1), 60_000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    if (sessions.length === 0) return;
    const n = callCount(TAG, "predict");
    const t0 = performance.now();
    invoke<PredictionResult>("predict_sleep", {
      sessions,
      nowIso: bedTimeToIso(bedTime),
    })
      .then(r => {
        setResult(r);
        const ms = Math.round(performance.now() - t0);
        if (ms > 100) {
          console.log(TAG, `predict #${n}: ${formatDuration(r.duration_hours)}  (+${ms}ms)`);
        }
      })
      .catch(e => console.error(TAG, `ERROR predict #${n}:`, e));
  }, [sessions, bedTime, tick]);

  function setToNow() {
    setBedTime(currentHHMM());
  }

  async function calcOptimal() {
    if (sessions.length === 0) return;
    setLoadingOptimal(true);
    try {
      const r = await invoke<OptimalResult | null>("find_optimal_bedtime", {
        sessions,
        nowIso: currentNowIso(),
      });
      if (r) setBedTime(r.best_bed_time);
    } catch (e) {
      console.error(TAG, "ERROR find_optimal_bedtime:", e);
    } finally {
      setLoadingOptimal(false);
    }
  }

  const wakeTime = result ? addHoursToHHMM(bedTime, result.duration_hours) : "--:--";

  return (
    <div className="pred-home-section">
      <div className="pred-home-ctrl">
        <span className="strip-label">睡眠<br />予測</span>

        <TimePicker value={bedTime} onChange={setBedTime} />
        <button className="pred-home-btn" onClick={setToNow}>今すぐ</button>
        <button
          className="pred-home-btn pred-home-opt"
          onClick={calcOptimal}
          disabled={loadingOptimal || sessions.length === 0}
        >
          {loadingOptimal ? "計算中..." : <>最適<br />睡眠</>}
        </button>

        {result && (
          <>
            <div className="strip-divider" />

            <div className="strip-col">
              <div className="big-value blue">{formatDuration(result.duration_hours)}</div>
              <div className="small-label">予測睡眠時間</div>
            </div>

            <div className="strip-col">
              <div className="big-value" style={{ whiteSpace: "nowrap" }}>{bedTime} 入眠 → {wakeTime} 起床</div>
            </div>

            <div className="strip-divider" />

            <div className="strip-col">
              <div className="small-label">起きてから</div>
              <div className="big-value" style={{ color: awakeColor(result.awake_hours) }}>
                {formatDuration(result.awake_hours)}
              </div>
            </div>

            <div className="strip-divider" />

            <div className="strip-col method-col">
              <div className="method-text">{result.method}</div>
            </div>
          </>
        )}

        {!result && sessions.length === 0 && (
          <div className="no-data-strip">データなし</div>
        )}
      </div>

    </div>
  );
}
