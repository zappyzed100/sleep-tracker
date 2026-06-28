import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Session } from "./types";
import { formatDuration } from "./utils";

interface PredictionResult {
  duration_hours: number;
  method: string;
  awake_hours: number;
}

interface OptimalResult {
  best_bed_time: string;
  min_duration_hours: number;
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
  const [optimal, setOptimal] = useState<OptimalResult | null>(null);
  const [loadingOptimal, setLoadingOptimal] = useState(false);

  useEffect(() => {
    if (sessions.length === 0) return;
    invoke<PredictionResult>("predict_sleep", {
      sessions,
      nowIso: bedTimeToIso(bedTime),
    })
      .then(setResult)
      .catch(console.error);
  }, [sessions, bedTime]);

  function setToNow() {
    setBedTime(currentHHMM());
    setOptimal(null);
  }

  async function calcOptimal() {
    if (sessions.length === 0) return;
    setLoadingOptimal(true);
    try {
      const r = await invoke<OptimalResult | null>("find_optimal_bedtime", {
        sessions,
        nowIso: currentNowIso(),
      });
      if (r) {
        setOptimal(r);
        setBedTime(r.best_bed_time);
      }
    } catch (e) {
      console.error(e);
    } finally {
      setLoadingOptimal(false);
    }
  }

  const wakeTime = result ? addHoursToHHMM(bedTime, result.duration_hours) : "--:--";

  return (
    <div className="pred-home-section">
      <div className="pred-home-ctrl">
        <span className="strip-label">睡眠<br />予測</span>

        <input
          type="time"
          className="pred-home-time"
          value={bedTime}
          onChange={(e) => { setBedTime(e.target.value); setOptimal(null); }}
        />
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

      {optimal && (
        <div className="pred-optimal-bar">
          <span>
            ★ <strong>{optimal.best_bed_time}</strong> に入眠すると最短睡眠{" "}
            <strong>{formatDuration(optimal.min_duration_hours)}</strong> で起きられます
          </span>
          <button onClick={() => setBedTime(optimal.best_bed_time)}>
            {optimal.best_bed_time} に設定
          </button>
        </div>
      )}
    </div>
  );
}
