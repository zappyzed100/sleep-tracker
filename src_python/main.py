# File: src_python/main.py
# Description: GUI application for viewing sleep history (with premium custom calendar popup, weekly navigation, and detail dialog) and predictions, with automatic GitHub connection warnings.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: SleepTrackerApp, CustomCalendar, SleepSessionDetailDialog, main
# Dependencies: tkinter, messagebox, matplotlib, pandas, datetime, calendar, database, analyzer, urllib.request, threading

import tkinter as tk
from tkinter import ttk, messagebox
from datetime import datetime, timedelta
import os
import matplotlib
matplotlib.use("TkAgg")
matplotlib.rcParams['font.family'] = ['Yu Gothic', 'Meiryo', 'MS Gothic', 'sans-serif']
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
import pandas as pd
import calendar
import urllib.request
import json
import threading

import database
import analyzer

class CustomCalendar(tk.Toplevel):
    """プレミアムな外観を持つフラットデザインのカスタムカレンダーポップアップ"""
    def __init__(self, parent, current_date, callback):
        super().__init__(parent)
        self.title("日付の選択")
        self.configure(bg="#1e1e2e")
        self.transient(parent)
        self.grab_set()
        
        self.callback = callback
        self.year = current_date.year
        self.month = current_date.month
        self.selected_day = current_date.day
        
        x = parent.winfo_x() + 450
        y = parent.winfo_y() + 180
        self.geometry(f"+{x}+{y}")
        self.resizable(False, False)
        
        try:
            ico_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico")
            if os.path.exists(ico_path):
                self.iconbitmap(ico_path)
        except Exception:
            pass
        
        header_frame = tk.Frame(self, bg="#1e1e2e")
        header_frame.pack(fill="x", padx=15, pady=12)
        
        prev_btn = tk.Button(
            header_frame, text="◀", font=("Yu Gothic UI", 10, "bold"), 
            bg="#313244", fg="#cdd6f4", activebackground="#45475a", activeforeground="#cdd6f4",
            bd=0, relief="flat", width=3, cursor="hand2"
        )
        prev_btn.pack(side="left", command=self.prev_month)
        
        self.title_label = tk.Label(header_frame, text="", font=("Yu Gothic UI", 12, "bold"), bg="#1e1e2e", fg="#89b4fa")
        self.title_label.pack(side="left", expand=True)
        
        next_btn = tk.Button(
            header_frame, text="▶", font=("Yu Gothic UI", 10, "bold"), 
            bg="#313244", fg="#cdd6f4", activebackground="#45475a", activeforeground="#cdd6f4",
            bd=0, relief="flat", width=3, cursor="hand2"
        )
        next_btn.pack(side="right", command=self.next_month)
        
        week_frame = tk.Frame(self, bg="#1e1e2e")
        week_frame.pack(fill="x", padx=15, pady=(5, 2))
        weekdays = ["月", "火", "水", "木", "金", "土", "日"]
        for w in weekdays:
            lbl = tk.Label(week_frame, text=w, font=("Yu Gothic UI", 9, "bold"), bg="#1e1e2e", fg="#a6adc8", width=4, height=1)
            lbl.pack(side="left", padx=3)
            
        self.grid_frame = tk.Frame(self, bg="#1e1e2e")
        self.grid_frame.pack(padx=15, pady=(2, 15))
        
        self.draw_calendar()
        
    def draw_calendar(self):
        for widget in self.grid_frame.winfo_children():
            widget.destroy()
            
        self.title_label.config(text=f"{self.year}年 {self.month}月")
        cal = calendar.Calendar(firstweekday=0)
        month_days = cal.monthdayscalendar(self.year, self.month)
        
        for r_idx, week in enumerate(month_days):
            for c_idx, day in enumerate(week):
                if day == 0:
                    lbl = tk.Label(self.grid_frame, text="", bg="#1e1e2e", width=4, height=2)
                    lbl.grid(row=r_idx, column=c_idx, padx=3, pady=3)
                else:
                    is_selected = (day == self.selected_day)
                    bg_color = "#89b4fa" if is_selected else "#252538"
                    fg_color = "#1e1e2e" if is_selected else "#cdd6f4"
                    
                    btn = tk.Button(
                        self.grid_frame, 
                        text=str(day), 
                        font=("Yu Gothic UI", 9, "bold"),
                        bg=bg_color, 
                        fg=fg_color, 
                        bd=0, 
                        activebackground="#45475a", 
                        activeforeground="#cdd6f4",
                        width=4, 
                        height=2,
                        cursor="hand2",
                        command=lambda d=day: self.select_day(d)
                    )
                    btn.grid(row=r_idx, column=c_idx, padx=3, pady=3)
                    
    def prev_month(self):
        if self.month == 1:
            self.month = 12
            self.year -= 1
        else:
            self.month -= 1
        self.selected_day = 1
        self.draw_calendar()
        
    def next_month(self):
        if self.month == 12:
            self.month = 1
            self.year += 1
        else:
            self.month += 1
        self.selected_day = 1
        self.draw_calendar()
        
    def select_day(self, day):
        selected_date = f"{self.year}-{self.month:02d}-{day:02d}"
        self.callback(selected_date)
        self.destroy()


class SleepSessionDetailDialog(tk.Toplevel):
    """選択された日の睡眠記録を表示・削除・手動追加するプレミアムダイアログ"""
    def __init__(self, parent, target_date, on_update_callback):
        super().__init__(parent)
        self.title("睡眠記録の詳細・編集")
        self.configure(bg="#1e1e2e")
        self.transient(parent)
        self.grab_set()
        
        self.target_date = target_date
        self.on_update_callback = on_update_callback
        
        # 画面サイズと位置
        self.geometry("520x620")
        x = parent.winfo_x() + 200
        y = parent.winfo_y() + 80
        self.geometry(f"+{x}+{y}")
        self.resizable(False, False)
        
        try:
            ico_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico")
            if os.path.exists(ico_path):
                self.iconbitmap(ico_path)
        except Exception:
            pass
            
        self.create_widgets()
        self.refresh_session_list()
        
    def create_widgets(self):
        # 1. タイトルヘッダー
        date_str = self.target_date.strftime("%Y年 %m月 %d日")
        weekday_str = ["月", "火", "水", "木", "金", "土", "日"][self.target_date.weekday()]
        header = tk.Label(self, text=f"🌙 {date_str} ({weekday_str}) の睡眠詳細", font=("Yu Gothic UI", 14, "bold"), bg="#1e1e2e", fg="#f9e2af")
        header.pack(fill="x", padx=20, pady=15)
        
        # 2. セッション一覧エリア (スクロール付き)
        list_label = tk.Label(self, text="睡眠セッション一覧 (開始時間基準):", font=("Yu Gothic UI", 10, "bold"), bg="#1e1e2e", fg="#bac2de")
        list_label.pack(anchor="w", padx=20, pady=(5, 2))
        
        self.list_frame = tk.Frame(self, bg="#252538", bd=1, relief="solid")
        self.list_frame.pack(fill="both", expand=True, padx=20, pady=5)
        
        # 3. 手動追加エリア
        add_frame = tk.LabelFrame(self, text="睡眠記録を手動で追加", font=("Yu Gothic UI", 10, "bold"), bg="#1e1e2e", fg="#a6e3a1", bd=1, labelanchor="n")
        add_frame.pack(fill="x", padx=20, pady=(15, 20))
        
        # 時刻入力部分のグリッド
        grid = tk.Frame(add_frame, bg="#1e1e2e")
        grid.pack(padx=15, pady=10)
        
        # --- 入眠時間 ---
        tk.Label(grid, text="入眠 (寝たとき):", bg="#1e1e2e", fg="#cdd6f4").grid(row=0, column=0, sticky="w", pady=5)
        self.start_date_var = tk.StringVar(value=self.target_date.strftime("%Y-%m-%d"))
        start_date_entry = tk.Entry(grid, textvariable=self.start_date_var, width=12, state="readonly", font=("Yu Gothic UI", 9, "bold"))
        start_date_entry.grid(row=0, column=1, padx=5)
        
        start_cal_btn = ttk.Button(grid, text="📅", width=3, command=lambda: self.open_calendar_for(self.start_date_var))
        start_cal_btn.grid(row=0, column=2, padx=2)
        
        self.start_hour = ttk.Combobox(grid, values=[f"{i:02d}" for i in range(24)], width=4, state="readonly")
        self.start_hour.set("23")
        self.start_hour.grid(row=0, column=3, padx=2)
        tk.Label(grid, text="時", bg="#1e1e2e", fg="#cdd6f4").grid(row=0, column=4)
        
        self.start_min = ttk.Combobox(grid, values=[f"{i:02d}" for i in range(60)], width=4, state="readonly")
        self.start_min.set("00")
        self.start_min.grid(row=0, column=5, padx=2)
        tk.Label(grid, text="分", bg="#1e1e2e", fg="#cdd6f4").grid(row=0, column=6)
        
        # --- 起床時間 ---
        tk.Label(grid, text="起床 (起きたとき):", bg="#1e1e2e", fg="#cdd6f4").grid(row=1, column=0, sticky="w", pady=5)
        self.end_date_var = tk.StringVar(value=(self.target_date + timedelta(days=1)).strftime("%Y-%m-%d"))
        end_date_entry = tk.Entry(grid, textvariable=self.end_date_var, width=12, state="readonly", font=("Yu Gothic UI", 9, "bold"))
        end_date_entry.grid(row=1, column=1, padx=5)
        
        end_cal_btn = ttk.Button(grid, text="📅", width=3, command=lambda: self.open_calendar_for(self.end_date_var))
        end_cal_btn.grid(row=1, column=2, padx=2)
        
        self.end_hour = ttk.Combobox(grid, values=[f"{i:02d}" for i in range(24)], width=4, state="readonly")
        self.end_hour.set("07")
        self.end_hour.grid(row=1, column=3, padx=2)
        tk.Label(grid, text="時", bg="#1e1e2e", fg="#cdd6f4").grid(row=1, column=4)
        
        self.end_min = ttk.Combobox(grid, values=[f"{i:02d}" for i in range(60)], width=4, state="readonly")
        self.end_min.set("00")
        self.end_min.grid(row=1, column=5, padx=2)
        tk.Label(grid, text="分", bg="#1e1e2e", fg="#cdd6f4").grid(row=1, column=6)
        
        # 追加実行ボタン
        add_btn = ttk.Button(add_frame, text="この睡眠データを手動追加する", command=self.add_session)
        add_btn.pack(pady=(0, 10))
        
    def open_calendar_for(self, var_target):
        try:
            curr = datetime.strptime(var_target.get(), "%Y-%m-%d")
        except Exception:
            curr = datetime.now()
        CustomCalendar(self, curr, lambda date_str: var_target.set(date_str))
        
    def refresh_session_list(self):
        for widget in self.list_frame.winfo_children():
            widget.destroy()
            
        sessions = database.get_sessions_with_ids()
        
        # 選択された日付に開始した、またはその日に属するセッションを抽出
        target_str = self.target_date.strftime("%Y-%m-%d")
        day_sessions = []
        for s_id, s_time, e_time, dur, s_type in sessions:
            if s_time.startswith(target_str):
                day_sessions.append((s_id, s_time, e_time, dur, s_type))
                
        if not day_sessions:
            lbl = tk.Label(self.list_frame, text="この日の睡眠記録はありません。", font=("Yu Gothic", 10), bg="#252538", fg="#a6adc8")
            lbl.pack(expand=True, fill="both", pady=30)
            return
            
        # リストアイテムを構築
        for s_id, s_time, e_time, dur, s_type in day_sessions:
            item_frame = tk.Frame(self.list_frame, bg="#252538")
            item_frame.pack(fill="x", padx=10, pady=5)
            
            # 時間部分のパースと整形表示
            st_time = s_time.split(" ")[1][:5]
            ed_time = e_time.split(" ")[1][:5] if e_time else "--:--"
            h = int(dur)
            m = int((dur % 1) * 60)
            type_lbl = " [外出]" if s_type == "out" else ""
            
            text_str = f"⏰ {st_time} 〜 {ed_time} ({h}時間{m}分){type_lbl}"
            lbl = tk.Label(item_frame, text=text_str, font=("Yu Gothic UI", 10, "bold"), bg="#252538", fg="#cdd6f4")
            lbl.pack(side="left", padx=5)
            
            del_btn = tk.Button(
                item_frame, text="削除", font=("Yu Gothic UI", 9, "bold"),
                bg="#f38ba8", fg="#11111b", activebackground="#eba0b2", activeforeground="#11111b",
                bd=0, padx=8, pady=2, cursor="hand2", command=lambda idx=s_id: self.delete_session(idx)
            )
            del_btn.pack(side="right", padx=5)
            
    def delete_session(self, session_id):
        if messagebox.askyesno("削除確認", "この睡眠データを完全に削除しますか？\n( sleep_events.txt を再構成し、GitHubへプッシュします)", icon="warning"):
            if database.delete_session_and_rebuild(session_id):
                messagebox.showinfo("成功", "睡眠データを削除し、同期しました。")
                self.refresh_session_list()
                self.on_update_callback()
            else:
                messagebox.showerror("エラー", "データの削除に失敗しました。")
                
    def add_session(self):
        st_date = self.start_date_var.get()
        st_time = f"{self.start_hour.get()}:{self.start_min.get()}:00"
        start_datetime = f"{st_date} {st_time}"
        
        ed_date = self.end_date_var.get()
        ed_time = f"{self.end_hour.get()}:{self.end_min.get()}:00"
        end_datetime = f"{ed_date} {ed_time}"
        
        success, message = database.add_session_and_rebuild(start_datetime, end_datetime, "sleep")
        if success:
            messagebox.showinfo("成功", "睡眠データを手動追加し、同期しました。")
            self.refresh_session_list()
            self.on_update_callback()
        else:
            messagebox.showerror("追加失敗", f"データの追加に失敗しました:\n{message}")


class SleepTrackerApp:
    def __init__(self, root):
        self.root = root
        self.root.title("睡眠トラッカー ＆ 予測ツール")
        self.root.geometry("950x820")
        self.root.configure(bg="#1e1e2e")

        try:
            ico_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico")
            if os.path.exists(ico_path):
                self.root.iconbitmap(ico_path)
        except Exception:
            pass

        database.init_db()
        try:
            database.sync_logs_to_db()
        except Exception:
            pass
        
        self.sessions = database.get_all_sessions()
        
        now = datetime.now()
        self.current_week_start = self.get_week_start_monday(now)
        
        self.style = ttk.Style()
        self.style.theme_use("clam")
        self.style.configure(".", background="#1e1e2e", foreground="#cdd6f4")
        self.style.configure("TLabel", background="#1e1e2e", foreground="#cdd6f4", font=("Yu Gothic", 11))
        self.style.configure("Card.TFrame", background="#252538", relief="flat")
        self.style.configure("TButton", font=("Yu Gothic UI", 10, "bold"), background="#313244", foreground="#cdd6f4")
        self.style.map("TButton",
            background=[('active', '#45475a'), ('pressed', '#585b70')],
            foreground=[('active', '#cdd6f4')]
        )
        
        self.create_widgets()
        self.periodic_connection_check()

    def get_week_start_monday(self, dt: datetime) -> datetime:
        return (dt - timedelta(days=dt.weekday())).replace(hour=0, minute=0, second=0, microsecond=0)

    def check_monitor_status(self) -> tuple[bool, str]:
        hb_info = database.read_last_heartbeat()
        if not hb_info:
            return False, "停止中 (生存信号なし)"
        
        hb_time, _ = hb_info
        if datetime.now() - hb_time < timedelta(minutes=3):
            return True, f"稼働中 (最終更新: {hb_time.strftime('%H:%M:%S')})"
        else:
            return False, f"停止中 (最終更新: {hb_time.strftime('%m-%d %H:%M')})"

    def create_widgets(self):
        title_frame = tk.Frame(self.root, bg="#1e1e2e")
        title_frame.pack(fill="x", padx=25, pady=(15, 10))
        
        title_label = tk.Label(title_frame, text="睡眠トラッカー", font=("Yu Gothic UI", 22, "bold"), bg="#1e1e2e", fg="#89b4fa")
        title_label.pack(side="left")
        
        is_running, status_text = self.check_monitor_status()
        status_color = "#a6e3a1" if is_running else "#f38ba8"
        status_label = tk.Label(title_frame, text=f"監視サービス: {status_text}", font=("Yu Gothic UI", 10, "bold"), bg="#1e1e2e", fg=status_color)
        status_label.pack(side="right", pady=8)

        self.warning_frame = tk.Frame(self.root, bg="#f38ba8", bd=1, relief="solid")
        self.warning_label = tk.Label(self.warning_frame, text="", font=("Yu Gothic UI", 10, "bold"), bg="#f38ba8", fg="#11111b")
        self.warning_label.pack(fill="x", padx=15, pady=6)

        self.summary_frame = tk.Frame(self.root, bg="#1e1e2e")
        self.summary_frame.pack(fill="x", padx=25, pady=5)
        
        self.pred_card = ttk.Frame(self.summary_frame, style="Card.TFrame")
        self.pred_card.pack(side="left", fill="both", expand=True, padx=(0, 10))
        
        self.stats_card = ttk.Frame(self.summary_frame, style="Card.TFrame")
        self.stats_card.pack(side="right", fill="both", expand=True, padx=(10, 0))

        self.update_prediction_and_stats()

        nav_frame = tk.Frame(self.root, bg="#1e1e2e")
        nav_frame.pack(fill="x", padx=25, pady=(15, 5))

        prev_btn = ttk.Button(nav_frame, text="◀ 前の週", command=self.go_to_prev_week)
        prev_btn.pack(side="left", padx=5)

        self.week_label = tk.Label(nav_frame, text="", font=("Yu Gothic UI", 13, "bold"), bg="#1e1e2e", fg="#cdd6f4")
        self.week_label.pack(side="left", expand=True)

        next_btn = ttk.Button(nav_frame, text="次の週 ▶", command=self.go_to_next_week)
        next_btn.pack(side="right", padx=5)

        cal_label = tk.Label(nav_frame, text="日付選択: ", font=("Yu Gothic", 10), bg="#1e1e2e", fg="#a6adc8")
        cal_label.pack(side="right", padx=(10, 2))
        
        self.date_var = tk.StringVar(value=self.current_week_start.strftime("%Y-%m-%d"))
        self.date_entry = tk.Entry(
            nav_frame, 
            textvariable=self.date_var, 
            width=12, 
            bg="white", 
            fg="black", 
            insertbackground="black", 
            font=("Yu Gothic UI", 10, "bold"), 
            bd=1, 
            relief="solid",
            state="readonly"
        )
        self.date_entry.pack(side="right", padx=2)
        
        cal_btn = ttk.Button(nav_frame, text="📅", width=3, command=self.open_calendar_popup)
        cal_btn.pack(side="right", padx=5)

        self.graph_frame = ttk.Frame(self.root, style="Card.TFrame")
        self.graph_frame.pack(fill="both", expand=True, padx=25, pady=(5, 25))
        
        self.canvas = None
        self.update_week_view()

    def open_calendar_popup(self):
        try:
            current_date = datetime.strptime(self.date_var.get(), "%Y-%m-%d")
        except Exception:
            current_date = datetime.now()
        CustomCalendar(self.root, current_date, self.on_date_selected_from_popup)

    def on_date_selected_from_popup(self, date_str):
        self.date_var.set(date_str)
        selected_dt = datetime.strptime(date_str, "%Y-%m-%d")
        self.current_week_start = self.get_week_start_monday(selected_dt)
        self.update_week_view()

    def update_prediction_and_stats(self):
        self.sessions = database.get_all_sessions() # 最新セッションを再取得
        now = datetime.now()
        pred_duration, pred_method = analyzer.predict_sleep_duration(self.sessions, now)
        pred_wake_time = now + timedelta(hours=pred_duration)
        
        for widget in self.pred_card.winfo_children():
            widget.destroy()
            
        tk.Label(self.pred_card, text="睡眠時間の予測", font=("Yu Gothic UI", 11, "bold"), bg="#252538", fg="#bac2de").pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(self.pred_card, text=f"今眠った場合の予測 ({now.strftime('%H:%M')} 入眠と仮定):", font=("Yu Gothic", 10), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15)
        
        pred_time_str = f"{int(pred_duration)}時間 {int((pred_duration % 1) * 60)}分"
        tk.Label(self.pred_card, text=pred_time_str, font=("Yu Gothic UI", 24, "bold"), bg="#252538", fg="#f9e2af").pack(anchor="w", padx=15, pady=2)
        
        method_ja = pred_method.replace("Heuristic", "簡易統計").replace("Machine Learning", "機械学習").replace("Awake Duration", "連続覚醒時間")
        tk.Label(self.pred_card, text=f"予測起床時刻: {pred_wake_time.strftime('%H:%M')}頃 ({method_ja})", font=("Yu Gothic Italic", 9), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15, pady=(0, 10))

        avg_sleep = 0.0
        last_sleep = 0.0
        total_days = len(self.sessions)
        if total_days > 0:
            df = pd.DataFrame(self.sessions, columns=['start', 'end', 'dur', 'type'])
            avg_sleep = df['dur'].mean()
            last_sleep = df['dur'].iloc[-1]
            
        for widget in self.stats_card.winfo_children():
            widget.destroy()
            
        tk.Label(self.stats_card, text="睡眠の統計", font=("Yu Gothic UI", 11, "bold"), bg="#252538", fg="#bac2de").pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(self.stats_card, text=f"合計記録日数: {total_days} 日", font=("Yu Gothic", 10), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15)
        
        avg_str = f"平均睡眠時間: {int(avg_sleep)}時間 {int((avg_sleep % 1) * 60)}分"
        tk.Label(self.stats_card, text=avg_str, font=("Yu Gothic UI", 15, "bold"), bg="#252538", fg="#a6e3a1").pack(anchor="w", padx=15, pady=2)
        
        last_str = f"前回の睡眠時間: {int(last_sleep)}時間 {int((last_sleep % 1) * 60)}分" if total_days > 0 else "前回の睡眠時間: 記録なし"
        tk.Label(self.stats_card, text=last_str, font=("Yu Gothic", 10), bg="#252538", fg="#cdd6f4").pack(anchor="w", padx=15, pady=(0, 10))

    def go_to_prev_week(self):
        self.current_week_start -= timedelta(days=7)
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def go_to_next_week(self):
        self.current_week_start += timedelta(days=7)
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def update_week_view(self):
        week_end = self.current_week_start + timedelta(days=6)
        label_text = f"{self.current_week_start.strftime('%Y/%m/%d')} (月)  〜  {week_end.strftime('%Y/%m/%d')} (日)"
        self.week_label.config(text=label_text)

        if self.canvas:
            self.canvas.get_tk_widget().destroy()
            
        self.plot_weekly_graph()
        self.update_prediction_and_stats() # 同期後の数値・統計も再計算

    def plot_weekly_graph(self):
        fig = Figure(figsize=(7, 4), dpi=100, facecolor="#252538")
        ax = fig.add_subplot(111)
        ax.set_facecolor("#252538")
        ax.grid(True, color="#313244", linestyle="--", linewidth=0.5)
        
        weekdays_ja = ['月', '火', '水', '木', '金', '土', '日']
        durations = [0.0] * 7
        days_in_week = [self.current_week_start + timedelta(days=i) for i in range(7)]
        xticklabels = [f"{w}\n({d.strftime('%m/%d')})" for w, d in zip(weekdays_ja, days_in_week)]
        
        for start_time_str, _, dur, _ in self.sessions:
            try:
                start_dt = datetime.strptime(start_time_str, "%Y-%m-%d %H:%M:%S")
                for idx, day in enumerate(days_in_week):
                    if start_dt.date() == day.date():
                        durations[idx] += dur
                        break
            except Exception:
                continue

        ax.spines['bottom'].color = '#45475a'
        ax.spines['left'].color = '#45475a'
        ax.spines['top'].visible = False
        ax.spines['right'].visible = False
        
        ax.set_xticks(range(7))
        ax.set_xticklabels(xticklabels, color='#bac2de', fontsize=9, fontproperties='Yu Gothic')
        
        ax.tick_params(colors='#bac2de', which='both', labelsize=10)
        ax.set_ylabel("睡眠時間 (時間)", color="#bac2de", fontsize=10, fontproperties='Yu Gothic')

        has_data = any(d > 0 for d in durations)
        if has_data:
            bars = ax.bar(weekdays_ja, durations, color="#89b4fa", width=0.55, edgecolor="#b4befe", linewidth=0.8, cursor="hand2")
            for bar in bars:
                height = bar.get_height()
                if height > 0:
                    ax.annotate(f'{height:.1f}h',
                                xy=(bar.get_x() + bar.get_width() / 2, height),
                                xytext=(0, 3),
                                textcoords="offset points",
                                ha='center', va='bottom', fontsize=8, color="#cdd6f4")
        else:
            ax.text(0.5, 0.5, "この週の睡眠ログデータはありません。\n(グラフ部分をクリックして睡眠データを手動登録できます)", 
                    ha="center", va="center", color="#a6adc8", fontsize=10, transform=ax.text(0,0,"").get_transform(), fontproperties='Yu Gothic')
            # プレースホルダーのダミーテキスト配置
            ax.text(0.5, 0.5, "この週の睡眠ログデータはありません。\n(グラフをクリックして手動で追加できます)", 
                    ha="center", va="center", color="#a6adc8", fontsize=10, transform=ax.transAxes, fontproperties='Yu Gothic')
            ax.set_ylim(0, 10)

        fig.tight_layout()
        self.canvas = FigureCanvasTkAgg(fig, master=self.graph_frame)
        self.canvas.draw()
        self.canvas.get_tk_widget().pack(fill="both", expand=True, padx=10, pady=(0, 10))
        
        # グラフ上クリックイベントをバインド
        self.canvas.mpl_connect("button_press_event", self.on_graph_click)

    def on_graph_click(self, event):
        """グラフをクリックした際に詳細ポップアップを開く"""
        if event.inaxes is None or event.xdata is None:
            return
        
        # xdata は X 軸の座標値 (曜日インデックス 0〜6)
        day_idx = int(round(event.xdata))
        if 0 <= day_idx < 7:
            target_date = self.current_week_start + timedelta(days=day_idx)
            SleepSessionDetailDialog(self.root, target_date, self.update_week_view)

    def show_connection_warning(self, reason: str):
        self.warning_label.config(text=f"⚠️ GitHub/Gistと同期できません ({reason})。ネット接続またはトークンを確認してください。")
        self.warning_frame.pack(fill="x", padx=25, pady=(5, 5), before=self.summary_frame)

    def hide_connection_warning(self):
        self.warning_frame.pack_forget()

    def check_github_connection(self):
        def run_test():
            base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
            config_path = os.path.join(base_dir, "config.json")
            if not os.path.exists(config_path):
                self.root.after(0, lambda: self.show_connection_warning("設定ファイル config.json なし"))
                return
                
            try:
                with open(config_path, "r", encoding="utf-8") as f:
                    config = json.load(f)
                    gist_id = config.get("gist_id")
                    token = config.get("github_token")
            except Exception:
                self.root.after(0, lambda: self.show_connection_warning("設定ファイルの読込失敗"))
                return
                
            if not gist_id or not token:
                self.root.after(0, lambda: self.show_connection_warning("Gist IDまたはトークン未設定"))
                return
                
            url = f"https://api.github.com/gists/{gist_id}"
            headers = {
                "Authorization": f"Bearer {token}",
                "Accept": "application/vnd.github+json",
                "User-Agent": "Sleep-Tracker-Client"
            }
            req = urllib.request.Request(url, headers=headers, method="GET")
            try:
                with urllib.request.urlopen(req, timeout=5) as response:
                    if response.status == 200:
                        self.root.after(0, self.hide_connection_warning)
                    else:
                        self.root.after(0, lambda: self.show_connection_warning(f"HTTP {response.status}"))
            except Exception as e:
                err_msg = str(e)
                if "401" in err_msg:
                    reason = "401 Unauthorized (トークン不正)"
                elif "404" in err_msg:
                    reason = "404 Not Found (Gist ID不正)"
                else:
                    reason = "接続タイムアウト / オフライン"
                self.root.after(0, lambda: self.show_connection_warning(reason))
                
        threading.Thread(target=run_test, daemon=True).start()

    def periodic_connection_check(self):
        self.check_github_connection()
        self.root.after(180000, self.periodic_connection_check)

def main():
    root = tk.Tk()
    app = SleepTrackerApp(root)
    root.mainloop()

if __name__ == "__main__":
    main()
