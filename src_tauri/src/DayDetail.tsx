import { useState, useEffect, useRef } from "react";
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

function fmtTs(ts: string): string {
  const [date, time] = ts.split(" ");
  const [, m, d] = date.split("-");
  return `${Number(m)}/${Number(d)} ${time.slice(0, 5)}`;
}

// ── SpinField ────────────────────────────────────────────────────────────────

function SpinField({ value, options, unit, onChange }: {
  value: string;
  options: string[];
  unit: string;
  onChange: (v: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const selectedRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    selectedRef.current?.scrollIntoView({ block: "center", behavior: "instant" });
    function handler(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  return (
    <div className="spin-field" ref={containerRef}>
      <button className="spin-value" onClick={() => setOpen(v => !v)}>
        {value}<span className="spin-unit">{unit}</span>
      </button>
      {open && (
        <div className="spin-list">
          {options.map(opt => (
            <div
              key={opt}
              ref={opt === value ? selectedRef : undefined}
              className={`spin-item${opt === value ? " active" : ""}`}
              onClick={() => { onChange(opt); setOpen(false); }}
            >
              {opt}{unit}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── DatePicker ───────────────────────────────────────────────────────────────

const HOUR_OPTS = Array.from({ length: 24 }, (_, i) => String(i).padStart(2, "0"));
const MIN_OPTS  = Array.from({ length: 60 }, (_, i) => String(i).padStart(2, "0"));

function DatePicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [year, month, day] = value.split("-");
  const curYear = new Date().getFullYear();
  const years  = Array.from({ length: 11 }, (_, i) => String(curYear - 5 + i));
  const months = Array.from({ length: 12 }, (_, i) => String(i + 1).padStart(2, "0"));
  const daysInMonth = new Date(Number(year), Number(month), 0).getDate();
  const days = Array.from({ length: daysInMonth }, (_, i) => String(i + 1).padStart(2, "0"));

  function update(y: string, m: string, d: string) {
    const max = new Date(Number(y), Number(m), 0).getDate();
    const clamped = Math.min(Number(d), max).toString().padStart(2, "0");
    onChange(`${y}-${m}-${clamped}`);
  }

  return (
    <div className="date-picker">
      <SpinField value={year}  options={years}  unit="年" onChange={y => update(y, month, day)} />
      <SpinField value={month} options={months} unit="月" onChange={m => update(year, m, day)} />
      <SpinField value={day}   options={days}   unit="日" onChange={d => update(year, month, d)} />
    </div>
  );
}

// ── DayDetail ────────────────────────────────────────────────────────────────

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
    const end   = `${endDate} ${endHH}:${endMM}:00`;
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
                    {fmtTs(s.start)} → {fmtTs(s.end)}
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
                  <DatePicker value={startDate} onChange={setStartDate} />
                  <SpinField value={startHH} options={HOUR_OPTS} unit="時" onChange={setStartHH} />
                  <SpinField value={startMM} options={MIN_OPTS}  unit="分" onChange={setStartMM} />
                </div>

                <div className="modal-add-row">
                  <span className="modal-add-label">起床</span>
                  <DatePicker value={endDate} onChange={setEndDate} />
                  <SpinField value={endHH} options={HOUR_OPTS} unit="時" onChange={setEndHH} />
                  <SpinField value={endMM} options={MIN_OPTS}  unit="分" onChange={setEndMM} />
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
