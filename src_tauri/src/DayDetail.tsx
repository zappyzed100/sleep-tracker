import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Session } from "./types";
import { formatDuration } from "./utils";

const DAYS_JA = ["日", "月", "火", "水", "木", "金", "土"];

function dateLabel(iso: string): string {
  const d = new Date(iso + "T12:00:00");
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日（${DAYS_JA[d.getDay()]}）`;
}

function nextDay(iso: string): string {
  const d = new Date(iso + "T12:00:00");
  d.setDate(d.getDate() + 1);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

const HOUR_OPTS = Array.from({ length: 24 }, (_, i) => String(i).padStart(2, "0"));
const MIN_OPTS = Array.from({ length: 60 }, (_, i) => String(i).padStart(2, "0"));

interface Props {
  date: string;
  sessions: Session[];
  onClose: () => void;
  onRefresh: () => void;
}

export default function DayDetail({ date, sessions, onClose, onRefresh }: Props) {
  const [addOpen, setAddOpen] = useState(false);
  const [startDate, setStartDate] = useState(date);
  const [startHH, setStartHH] = useState("23");
  const [startMM, setStartMM] = useState("00");
  const [endDate, setEndDate] = useState(() => nextDay(date));
  const [endHH, setEndHH] = useState("07");
  const [endMM, setEndMM] = useState("00");
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  // Escape key to close
  useEffect(() => {
    function onKey(e: KeyboardEvent) { if (e.key === "Escape") onClose(); }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const daySessions = sessions.filter((s) => s.start.startsWith(date));
  const totalHours = daySessions.reduce((sum, s) => sum + s.duration, 0);

  async function handleDelete(s: Session) {
    setError(null);
    setDeleting(s.start);
    try {
      await invoke("delete_session", { start: s.start, end: s.end });
      await onRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(null);
    }
  }

  async function handleAdd() {
    setError(null);
    const start = `${startDate} ${startHH}:${startMM}:00`;
    const end = `${endDate} ${endHH}:${endMM}:00`;
    if (start >= end) { setError("起床時刻は入眠時刻より後にしてください"); return; }
    try {
      await invoke("add_session", { start, end });
      await onRefresh();
      setAddOpen(false);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-card" onClick={(e) => e.stopPropagation()}>

        {/* Header */}
        <div className="modal-header">
          <span className="modal-title">🌙 {dateLabel(date)} の睡眠</span>
          <button className="modal-close" onClick={onClose}>✕</button>
        </div>

        <div className="modal-body">

          {/* Session list */}
          {daySessions.length > 0 ? (
            <>
              <div className="modal-total">合計 {formatDuration(totalHours)}</div>
              {daySessions.map((s, i) => (
                <div key={i} className="modal-session-row">
                  <span className="modal-session-time">
                    {s.start.slice(11, 16)} → {s.end.slice(11, 16)}
                  </span>
                  <span className="modal-session-dur">{formatDuration(s.duration)}</span>
                  <button
                    className="modal-delete-btn"
                    disabled={deleting === s.start}
                    onClick={() => handleDelete(s)}
                  >
                    {deleting === s.start ? "…" : "削除"}
                  </button>
                </div>
              ))}
            </>
          ) : (
            <div className="modal-no-data">この日の記録なし</div>
          )}

          {/* Add section */}
          <div className="modal-add-section">
            <button
              className="modal-add-toggle"
              onClick={() => { setAddOpen((v) => !v); setError(null); }}
            >
              {addOpen ? "▲ キャンセル" : "＋ 睡眠を手動追加"}
            </button>

            {addOpen && (
              <div className="modal-add-form">
                <div className="modal-add-row">
                  <span className="modal-add-label">入眠</span>
                  <input
                    type="date"
                    className="modal-date-input"
                    value={startDate}
                    onChange={(e) => setStartDate(e.target.value)}
                  />
                  <select className="modal-sel" value={startHH} onChange={(e) => setStartHH(e.target.value)}>
                    {HOUR_OPTS.map((h) => <option key={h}>{h}</option>)}
                  </select>
                  <span className="modal-add-unit">時</span>
                  <select className="modal-sel" value={startMM} onChange={(e) => setStartMM(e.target.value)}>
                    {MIN_OPTS.map((m) => <option key={m}>{m}</option>)}
                  </select>
                  <span className="modal-add-unit">分</span>
                </div>

                <div className="modal-add-row">
                  <span className="modal-add-label">起床</span>
                  <input
                    type="date"
                    className="modal-date-input"
                    value={endDate}
                    onChange={(e) => setEndDate(e.target.value)}
                  />
                  <select className="modal-sel" value={endHH} onChange={(e) => setEndHH(e.target.value)}>
                    {HOUR_OPTS.map((h) => <option key={h}>{h}</option>)}
                  </select>
                  <span className="modal-add-unit">時</span>
                  <select className="modal-sel" value={endMM} onChange={(e) => setEndMM(e.target.value)}>
                    {MIN_OPTS.map((m) => <option key={m}>{m}</option>)}
                  </select>
                  <span className="modal-add-unit">分</span>
                </div>

                <button className="modal-add-btn" onClick={handleAdd}>追加する</button>
              </div>
            )}
          </div>

          {error && <div className="modal-error">{error}</div>}
        </div>
      </div>
    </div>
  );
}
