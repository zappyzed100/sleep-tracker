// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// types.ts — アプリ全体で共有する型定義
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : Session・DaySummary など Rust バックエンドとやりとりする共有型を定義する
//
// 公開 : Session, DaySummary
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface Session {
  start: string;
  end: string;
  duration: number;
  type: string;
}

export interface DaySummary {
  date: string;
  totalHours: number;
  sessions: Session[];
  bedtimeH: number | null;
  waketimeH: number | null;
}
