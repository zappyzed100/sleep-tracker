# File: src_python/main.py
# Description: GUI application for viewing sleep history, prediction, and monitor status.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: SleepTrackerApp, main
# Dependencies: tkinter, matplotlib, pandas, datetime, database, analyzer

import tkinter as tk
from tkinter import ttk
from datetime import datetime, timedelta
import os
import matplotlib
matplotlib.use("TkAgg")
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
import pandas as pd

import database
import analyzer

class SleepTrackerApp:
    def __init__(self, root):
        self.root = root
        self.root.title("Sleep Tracker & Predictor")
        self.root.geometry("900x700")
        self.root.configure(bg="#1e1e2e") # カプチーノ/ダークテーマ背景
        
        # データベースの初期化とログの同期
        database.init_db()
        database.sync_logs_to_db()
        
        self.sessions = database.get_all_sessions()
        
        # スタイル設定
        self.style = ttk.Style()
        self.style.theme_use("clam")
        self.style.configure(".", background="#1e1e2e", foreground="#cdd6f4")
        self.style.configure("TLabel", background="#1e1e2e", foreground="#cdd6f4", font=("Segoe UI", 11))
        self.style.configure("Card.TFrame", background="#252538", relief="flat")
        
        self.create_widgets()

    def check_monitor_status(self) -> tuple[bool, str]:
        """監視サービスが稼働しているかをハートビートファイルの更新日時から確認する"""
        hb_info = database.read_last_heartbeat()
        if not hb_info:
            return False, "Not Running (No Heartbeat)"
        
        hb_time, _ = hb_info
        # 最終ハートビートが3分以内ならアクティブと判断
        if datetime.now() - hb_time < timedelta(minutes=3):
            return True, f"Active (Last update: {hb_time.strftime('%H:%M:%S')})"
        else:
            return False, f"Stopped (Last active: {hb_time.strftime('%m-%d %H:%M')})"

    def create_widgets(self):
        # 1. タイトル＆ステータスバー
        title_frame = tk.Frame(self.root, bg="#1e1e2e")
        title_frame.pack(fill="x", padx=20, pady=15)
        
        title_label = tk.Label(title_frame, text="SLEEP TRACKER", font=("Segoe UI Semibold", 20), bg="#1e1e2e", fg="#89b4fa")
        title_label.pack(side="left")
        
        is_running, status_text = self.check_monitor_status()
        status_color = "#a6e3a1" if is_running else "#f38ba8"
        status_label = tk.Label(title_frame, text=f"Monitor Service: {status_text}", font=("Segoe UI", 10, "bold"), bg="#1e1e2e", fg=status_color)
        status_label.pack(side="right", pady=5)

        # 2. 上部サマリーカードエリア（予測＆統計）
        summary_frame = tk.Frame(self.root, bg="#1e1e2e")
        summary_frame.pack(fill="x", padx=20, pady=5)
        
        # 予測カード
        pred_card = ttk.Frame(summary_frame, style="Card.TFrame")
        pred_card.pack(side="left", fill="both", expand=True, padx=(0, 10))
        
        # 予測の計算
        now = datetime.now()
        pred_duration, pred_method = analyzer.predict_sleep_duration(self.sessions, now)
        pred_wake_time = now + timedelta(hours=pred_duration)
        
        tk.Label(pred_card, text="SLEEP PREDICTION", font=("Segoe UI Semibold", 10), bg="#252538", fg="#bac2de").pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(pred_card, text=f"If you sleep now ({now.strftime('%H:%M')}):", font=("Segoe UI", 10), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15)
        
        pred_time_str = f"{int(pred_duration)}h {int((pred_duration % 1) * 60)}m"
        tk.Label(pred_card, text=pred_time_str, font=("Segoe UI Semibold", 22), bg="#252538", fg="#f9e2af").pack(anchor="w", padx=15, pady=2)
        tk.Label(pred_card, text=f"Expected Wake Up: {pred_wake_time.strftime('%H:%M')} ({pred_method})", font=("Segoe UI Italic", 9), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15, pady=(0, 10))

        # 統計カード
        stats_card = ttk.Frame(summary_frame, style="Card.TFrame")
        stats_card.pack(side="right", fill="both", expand=True, padx=(10, 0))
        
        avg_sleep = 0.0
        last_sleep = 0.0
        total_days = len(self.sessions)
        if total_days > 0:
            df = pd.DataFrame(self.sessions, columns=['start', 'end', 'dur', 'type'])
            avg_sleep = df['dur'].mean()
            last_sleep = df['dur'].iloc[-1]
            
        tk.Label(stats_card, text="SLEEP STATISTICS", font=("Segoe UI Semibold", 10), bg="#252538", fg="#bac2de").pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(stats_card, text=f"Total Logged Days: {total_days}", font=("Segoe UI", 10), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15)
        
        avg_str = f"Average: {int(avg_sleep)}h {int((avg_sleep % 1) * 60)}m"
        tk.Label(stats_card, text=avg_str, font=("Segoe UI Semibold", 14), bg="#252538", fg="#a6e3a1").pack(anchor="w", padx=15, pady=2)
        
        last_str = f"Last Session: {int(last_sleep)}h {int((last_sleep % 1) * 60)}m" if total_days > 0 else "Last Session: N/A"
        tk.Label(stats_card, text=last_str, font=("Segoe UI", 10), bg="#252538", fg="#cdd6f4").pack(anchor="w", padx=15, pady=(0, 10))

        # 3. グラフ表示エリア
        graph_frame = ttk.Frame(self.root, style="Card.TFrame")
        graph_frame.pack(fill="both", expand=True, padx=20, pady=15)
        
        tk.Label(graph_frame, text="SLEEP HISTORY (Last 14 Sessions)", font=("Segoe UI Semibold", 12), bg="#252538", fg="#cdd6f4").pack(anchor="w", padx=15, pady=10)
        
        self.plot_graph(graph_frame)

    def plot_graph(self, parent_frame):
        # グラフ作成
        fig = Figure(figsize=(7, 4), dpi=100, facecolor="#252538")
        ax = fig.add_subplot(111)
        ax.set_facecolor("#252538")
        
        # 目盛り線の描画
        ax.grid(True, color="#313244", linestyle="--", linewidth=0.5)
        
        # データの準備 (直近14個のセッション)
        recent_sessions = self.sessions[-14:] if len(self.sessions) > 0 else []
        
        if recent_sessions:
            dates = []
            durations = []
            for start, _, dur, _ in recent_sessions:
                dt = datetime.strptime(start, "%Y-%m-%d %H:%M:%S")
                # グラフに表示する日付フォーマット (MM/DD)
                dates.append(dt.strftime("%m/%d"))
                durations.append(dur)
            
            # 美しいグラデーションブルーの棒グラフを描画
            bars = ax.bar(dates, durations, color="#89b4fa", width=0.6, edgecolor="#b4befe", linewidth=0.8)
            
            # 棒の上に数値を表示
            for bar in bars:
                height = bar.get_height()
                ax.annotate(f'{height:.1f}h',
                            xy=(bar.get_x() + bar.get_width() / 2, height),
                            xytext=(0, 3),  # 3ポイント上にオフセット
                            textcoords="offset points",
                            ha='center', va='bottom', fontsize=8, color="#cdd6f4")
        else:
            # データがない場合のプレースホルダー
            ax.text(0.5, 0.5, "No Sleep Logs Recorded Yet.\nRun the C++ or Python monitor service to collect data.", 
                    ha="center", va="center", color="#a6adc8", fontsize=10, transform=ax.transAxes)
            ax.set_xticks([])
            ax.set_yticks([])
        
        # 軸と目盛りのスタイル調整
        ax.spines['bottom'].color = '#45475a'
        ax.spines['left'].color = '#45475a'
        ax.spines['top'].visible = False
        ax.spines['right'].visible = False
        ax.tick_params(colors='#bac2de', which='both', labelsize=9)
        ax.set_ylabel("Duration (Hours)", color="#bac2de", fontsize=9)
        
        # Tkinter キャンバスに統合
        canvas = FigureCanvasTkAgg(fig, master=parent_frame)
        canvas.draw()
        canvas.get_tk_widget().pack(fill="both", expand=True, padx=10, pady=(0, 10))

def main():
    root = tk.Tk()
    app = SleepTrackerApp(root)
    root.mainloop()

if __name__ == "__main__":
    main()
