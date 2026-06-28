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

function addHours(iso: string, hours: number): string {
  const d = new Date(iso.replace(" ", "T"));
  d.setTime(d.getTime() + hours * 3_600_000);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function awakeColor(h: number): string {
  if (h > 16) return "var(--red)";
  if (h > 12) return "var(--yellow)";
  return "var(--green)";
}

interface Props {
  sessions: Session[];
}

export default function PredictionTab({ sessions }: Props) {
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

  const bedIso = bedTimeToIso(bedTime);
  const wakeTime = result ? addHours(bedIso, result.duration_hours) : "--:--";

  return (
    <div className="pred-tab">
      <div className="pred-input-section">
        <div className="pred-section-title">入眠時刻</div>
        <div className="pred-input-row">
          <input
            type="time"
            className="pred-time-input"
            value={bedTime}
            onChange={(e) => { setBedTime(e.target.value); setOptimal(null); }}
          />
          <button className="pred-btn" onClick={setToNow}>今すぐ</button>
          <button
            className="pred-btn pred-optimal-btn"
            onClick={calcOptimal}
            disabled={loadingOptimal || sessions.length === 0}
          >
            {loadingOptimal ? "計算中..." : "最適睡眠"}
          </button>
        </div>
      </div>

      {result ? (
        <>
          <div className="pred-results-row">
            <div className="pred-stat-card">
              <div className="pred-stat-label">予測睡眠時間</div>
              <div className="pred-stat-value blue">{formatDuration(result.duration_hours)}</div>
            </div>
            <div className="pred-stat-card">
              <div className="pred-stat-label">起床予定</div>
              <div className="pred-stat-value green">{wakeTime}</div>
            </div>
            <div className="pred-stat-card">
              <div className="pred-stat-label">起床からの経過時間</div>
              <div className="pred-stat-value" style={{ color: awakeColor(result.awake_hours) }}>
                {formatDuration(result.awake_hours)}
              </div>
            </div>
          </div>

          <div className="pred-method">{result.method}</div>

          {optimal && (
            <div className="pred-optimal-banner">
              <span>
                ★ <strong>{optimal.best_bed_time}</strong> に入眠すると最短睡眠{" "}
                <strong>{formatDuration(optimal.min_duration_hours)}</strong> で起きられます
              </span>
              <button
                className="pred-optimal-set-btn"
                onClick={() => setBedTime(optimal.best_bed_time)}
              >
                {optimal.best_bed_time} に設定
              </button>
            </div>
          )}
        </>
      ) : (
        <div className="pred-no-data">データなし</div>
      )}
    </div>
  );
}
