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
