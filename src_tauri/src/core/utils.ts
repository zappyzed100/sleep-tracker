// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// utils.ts — 日付操作・週データ構築ユーティリティ
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 日付のパース・週の開始日計算・DaySummary 配列の構築など、
//        ドメインロジックに依存しない純粋関数を提供する
//
// 依存 : core/types
// 公開 : parseLocalDate, weekStart, addDays, isoDate, toNightHour,
//        buildWeek, formatDuration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import { Session, DaySummary } from "./types";

export function parseLocalDate(s: string): Date {
  const [datePart, timePart] = s.split(" ");
  const [y, mo, d] = datePart.split("-").map(Number);
  const [h, mi, sec] = (timePart ?? "00:00:00").split(":").map(Number);
  return new Date(y, mo - 1, d, h, mi, sec);
}

export function weekStart(ref: Date): Date {
  const d = new Date(ref);
  const day = d.getDay();
  const diff = day === 0 ? -6 : 1 - day;
  d.setDate(d.getDate() + diff);
  d.setHours(0, 0, 0, 0);
  return d;
}

export function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setDate(r.getDate() + n);
  return r;
}

export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function toNightHour(d: Date): number {
  const h = d.getHours() + d.getMinutes() / 60;
  return h < 12 ? h + 24 : h;
}

export function buildWeek(sessions: Session[], weekBase: Date): DaySummary[] {
  const start = weekStart(weekBase);
  return Array.from({ length: 7 }, (_, i) => {
    const day = addDays(start, i);
    const dateStr = isoDate(day);
    const next = addDays(day, 1);

    const daySessions = sessions.filter((s) => {
      const st = parseLocalDate(s.start);
      return st >= day && st < next;
    });

    if (daySessions.length === 0) {
      return { date: dateStr, totalHours: 0, sessions: [], bedtimeH: null, waketimeH: null };
    }

    const total = daySessions.reduce((sum, s) => sum + s.duration, 0);
    const longest = daySessions.reduce((a, b) => (a.duration > b.duration ? a : b));
    const bedtime = parseLocalDate(longest.start);
    const waketime = parseLocalDate(longest.end);

    return {
      date: dateStr,
      totalHours: total,
      sessions: daySessions,
      bedtimeH: toNightHour(bedtime),
      waketimeH: toNightHour(waketime),
    };
  });
}

export function formatDuration(hours: number): string {
  const h = Math.floor(hours);
  const m = Math.round((hours - h) * 60);
  return m > 0 ? `${h}h${m}m` : `${h}h`;
}
