# File: src_python/calendar_ui.py
# Description: Custom premium calendar popup dialog for selecting date in Tkinter Sleep Tracker.
# Date: 2026-06-27
# Author: Antigravity
# Main Classes: CustomCalendar
# Dependencies: tkinter, calendar, os, datetime

import tkinter as tk
import calendar
import os
from datetime import datetime

class CustomCalendar(tk.Toplevel):
    """プレミアムな外観を持つフラットデザイン of カスタムカレンダーポップアップ"""
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
        
        # ウィンドウ位置の調整 (親の近くに表示)
        x = parent.winfo_x() + 450
        y = parent.winfo_y() + 180
        self.geometry(f"+{x}+{y}")
        self.resizable(False, False)
        
        # ウィンドウアイコンの設定 (月アイコンがあれば適用)
        try:
            ico_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico")
            if os.path.exists(ico_path):
                self.iconbitmap(ico_path)
        except Exception:
            pass
        
        # ヘッダーコントロール
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
        
        # 曜日ヘッダー (月曜始まり)
        week_frame = tk.Frame(self, bg="#1e1e2e")
        week_frame.pack(fill="x", padx=15, pady=(5, 2))
        weekdays = ["月", "火", "水", "木", "金", "土", "日"]
        for w in weekdays:
            lbl = tk.Label(week_frame, text=w, font=("Yu Gothic UI", 9, "bold"), bg="#1e1e2e", fg="#a6adc8", width=4, height=1)
            lbl.pack(side="left", padx=3)
            
        # 日付グリッド
        self.grid_frame = tk.Frame(self, bg="#1e1e2e")
        self.grid_frame.pack(padx=15, pady=(2, 15))
        
        self.draw_calendar()
        
    def draw_calendar(self):
        # 既存の日付ボタンをクリア
        for widget in self.grid_frame.winfo_children():
            widget.destroy()
            
        self.title_label.config(text=f"{self.year}年 {self.month}月")
        
        # 月のカレンダーマトリクスを取得 (月曜始まり)
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
