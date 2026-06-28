import { useState, useEffect, useRef } from "react";
import { weekStart, addDays, isoDate } from "./utils";

const DAYS_JA = ["月", "火", "水", "木", "金", "土", "日"];

interface Props {
  current: Date;
  onSelect: (d: Date) => void;
  onClose: () => void;
  anchorRef: React.RefObject<HTMLButtonElement | null>;
}

export default function CalendarPicker({ current, onSelect, onClose, anchorRef }: Props) {
  const today = new Date();
  const [view, setView] = useState(() => new Date(current.getFullYear(), current.getMonth(), 1));
  const panelRef = useRef<HTMLDivElement>(null);

  // Close on outside click
  useEffect(() => {
    function onDown(e: MouseEvent) {
      const t = e.target as Node;
      if (!panelRef.current?.contains(t) && !anchorRef.current?.contains(t)) {
        onClose();
      }
    }
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose, anchorRef]);

  // Build grid: find Monday of the week containing 1st of view month
  const firstDay = new Date(view.getFullYear(), view.getMonth(), 1);
  const gridStart = weekStart(firstDay); // Monday

  const cells: Date[] = [];
  for (let i = 0; i < 42; i++) cells.push(addDays(gridStart, i));

  // Trim trailing empty rows
  let lastCellNeeded = 0;
  for (let i = cells.length - 1; i >= 0; i--) {
    if (cells[i].getMonth() === view.getMonth()) { lastCellNeeded = i; break; }
  }
  const rows = Math.ceil((lastCellNeeded + 1) / 7);
  const grid = cells.slice(0, rows * 7);

  const ws = weekStart(current);
  const we = addDays(ws, 6);

  function isCurrentWeek(d: Date) {
    const iso = isoDate(d);
    return iso >= isoDate(ws) && iso <= isoDate(we);
  }

  function prevMonth() {
    setView((v) => new Date(v.getFullYear(), v.getMonth() - 1, 1));
  }
  function nextMonth() {
    setView((v) => new Date(v.getFullYear(), v.getMonth() + 1, 1));
  }

  return (
    <div className="cal-panel" ref={panelRef}>
      <div className="cal-header">
        <button className="cal-nav-btn" onClick={prevMonth}>◀</button>
        <span className="cal-month-label">{view.getFullYear()}年{view.getMonth() + 1}月</span>
        <button className="cal-nav-btn" onClick={nextMonth}>▶</button>
      </div>

      <div className="cal-grid">
        {DAYS_JA.map((d) => (
          <div key={d} className="cal-dow">{d}</div>
        ))}
        {grid.map((d, i) => {
          const inMonth = d.getMonth() === view.getMonth();
          const isToday = isoDate(d) === isoDate(today);
          const inWeek = isCurrentWeek(d);
          return (
            <div
              key={i}
              className={[
                "cal-day",
                !inMonth ? "out" : "",
                isToday ? "today" : "",
                inWeek ? "in-week" : "",
              ].join(" ").trim()}
              onClick={() => { onSelect(d); onClose(); }}
            >
              {d.getDate()}
            </div>
          );
        })}
      </div>
    </div>
  );
}
