import { useRef, useEffect, useState } from "react";

const HOURS = Array.from({ length: 24 }, (_, i) => String(i).padStart(2, "0"));
const MINUTES = Array.from({ length: 60 }, (_, i) => String(i).padStart(2, "0"));

interface ColDropdownProps {
  values: string[];
  selected: string;
  onSelect: (v: string) => void;
  onClose: () => void;
  triggerRef: React.RefObject<HTMLSpanElement | null>;
}

function ColDropdown({ values, selected, onSelect, onClose, triggerRef }: ColDropdownProps) {
  const dropRef = useRef<HTMLDivElement>(null);
  const selectedIdx = values.indexOf(selected);
  const selectedIdxRef = useRef(selectedIdx);
  const onSelectRef = useRef(onSelect);

  useEffect(() => { selectedIdxRef.current = selectedIdx; }, [selectedIdx]);
  useEffect(() => { onSelectRef.current = onSelect; }, [onSelect]);

  // Close on outside click
  useEffect(() => {
    function onDown(e: MouseEvent) {
      const t = e.target as Node;
      if (!dropRef.current?.contains(t) && !triggerRef.current?.contains(t)) {
        onClose();
      }
    }
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose, triggerRef]);

  // Scroll selected into center on open
  useEffect(() => {
    const el = dropRef.current;
    if (!el) return;
    el.querySelectorAll<HTMLElement>(".tp-item")[selectedIdx]?.scrollIntoView({ block: "center" });
  }, []);

  // Wheel: navigate ±1, keep selection centered
  useEffect(() => {
    const el = dropRef.current;
    if (!el) return;
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      e.stopPropagation();
      const next = (selectedIdxRef.current + (e.deltaY > 0 ? 1 : -1) + values.length) % values.length;
      onSelectRef.current(values[next]);
      setTimeout(() => {
        dropRef.current?.querySelectorAll<HTMLElement>(".tp-item")[next]?.scrollIntoView({ block: "center", behavior: "smooth" });
      }, 0);
    }
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [values]);

  return (
    <div className="tp-dropdown" ref={dropRef}>
      {values.map((v, i) => (
        <div
          key={v}
          className={`tp-item${i === selectedIdx ? " selected" : ""}`}
          onClick={() => { onSelect(v); onClose(); }}
        >
          {v}
        </div>
      ))}
    </div>
  );
}

interface Props {
  value: string;
  onChange: (v: string) => void;
}

export default function TimePicker({ value, onChange }: Props) {
  const [active, setActive] = useState<"h" | "m" | null>(null);
  const hourRef = useRef<HTMLSpanElement>(null);
  const minRef = useRef<HTMLSpanElement>(null);

  const [hh, mm] = value.split(":");
  const hhRef = useRef(hh);
  const mmRef = useRef(mm);
  const onChangeRef = useRef(onChange);
  useEffect(() => { hhRef.current = hh; }, [hh]);
  useEffect(() => { mmRef.current = mm; }, [mm]);
  useEffect(() => { onChangeRef.current = onChange; }, [onChange]);

  // Wheel on hour trigger
  useEffect(() => {
    const el = hourRef.current;
    if (!el) return;
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      const next = (parseInt(hhRef.current) + (e.deltaY > 0 ? 1 : -1) + 24) % 24;
      onChangeRef.current(`${String(next).padStart(2, "0")}:${mmRef.current}`);
    }
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  // Wheel on minute trigger
  useEffect(() => {
    const el = minRef.current;
    if (!el) return;
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      const next = (parseInt(mmRef.current) + (e.deltaY > 0 ? 1 : -1) + 60) % 60;
      onChangeRef.current(`${hhRef.current}:${String(next).padStart(2, "0")}`);
    }
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  return (
    <div className="tp-root">
      {/* Hour column */}
      <span className="tp-col-wrap">
        <span
          ref={hourRef}
          className={`tp-col-trigger${active === "h" ? " active" : ""}`}
          onClick={() => setActive((a) => a === "h" ? null : "h")}
        >
          {hh}
        </span>
        {active === "h" && (
          <ColDropdown
            values={HOURS}
            selected={hh}
            onSelect={(v) => onChange(`${v}:${mm}`)}
            onClose={() => setActive(null)}
            triggerRef={hourRef}
          />
        )}
      </span>

      <span className="tp-colon">:</span>

      {/* Minute column */}
      <span className="tp-col-wrap">
        <span
          ref={minRef}
          className={`tp-col-trigger${active === "m" ? " active" : ""}`}
          onClick={() => setActive((a) => a === "m" ? null : "m")}
        >
          {mm}
        </span>
        {active === "m" && (
          <ColDropdown
            values={MINUTES}
            selected={mm}
            onSelect={(v) => onChange(`${hh}:${v}`)}
            onClose={() => setActive(null)}
            triggerRef={minRef}
          />
        )}
      </span>
    </div>
  );
}
