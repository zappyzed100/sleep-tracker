# File: src_python/main.py
# Description: GUI application for viewing sleep history (with premium custom calendar popup and weekly navigation) and predictions.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: SleepTrackerApp, CustomCalendar, main
# Dependencies: tkinter, matplotlib, pandas, datetime, calendar, database, analyzer

import tkinter as tk
from tkinter import ttk
from datetime import datetime, timedelta
import os
import sys
import subprocess
import threading
import matplotlib
matplotlib.use("TkAgg")
# Windows用の滑らかな日本語フォント (游ゴシック, メイリオ) を最優先に設定
matplotlib.rcParams['font.family'] = ['Yu Gothic', 'Meiryo', 'MS Gothic', 'sans-serif']
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
import pandas as pd
import calendar

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
        prev_btn.pack(side="left")
        prev_btn.config(command=self.prev_month)
        
        self.title_label = tk.Label(header_frame, text="", font=("Yu Gothic UI", 12, "bold"), bg="#1e1e2e", fg="#89b4fa")
        self.title_label.pack(side="left", expand=True)
        
        next_btn = tk.Button(
            header_frame, text="▶", font=("Yu Gothic UI", 10, "bold"), 
            bg="#313244", fg="#cdd6f4", activebackground="#45475a", activeforeground="#cdd6f4",
            bd=0, relief="flat", width=3, cursor="hand2"
        )
        next_btn.pack(side="right")
        next_btn.config(command=self.next_month)
        
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


class SleepTrackerApp:
    def __init__(self, root):
        self.root = root
        self.root.title("睡眠トラッカー ＆ 予測ツール")
        self.root.geometry("950x780")
        self.root.configure(bg="#1e1e2e") # ダークモード背景

        # ウィンドウのアイコンを設定
        try:
            ico_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico")
            if os.path.exists(ico_path):
                self.root.iconbitmap(ico_path)
        except Exception:
            pass

        # データベースの初期化
        database.init_db()

        # DB が空なら events.txt から同期実行で再構築 (新PC移植時など)
        # データがあれば非同期で更新 (Gist HTTP をブロックしないため)
        if not database.get_all_sessions():
            try:
                database.sync_logs_to_db()
            except Exception:
                pass
        else:
            try:
                threading.Thread(target=database.sync_logs_to_db, daemon=True).start()
            except Exception:
                pass

        self.sessions = database.get_all_sessions()
        
        # 表示中の週の月曜日を保持 (初期値は現在日付の週の月曜日)
        now = datetime.now()
        self.current_week_start = self.get_week_start_monday(now)
        
        # スタイル設定 (美しく滑らかな游ゴシック/Yu Gothic UIで統一)
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
        self._start_monitor_watchdog()

    def get_week_start_monday(self, dt: datetime) -> datetime:
        """指定された日時の週の月曜日 (00:00:00) を取得する"""
        return (dt - timedelta(days=dt.weekday())).replace(hour=0, minute=0, second=0, microsecond=0)

    def _start_monitor_watchdog(self):
        """モニターが落ちたら UI も自動で閉じる監視スレッドを起動する"""
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        pid_file = os.path.join(base_dir, "src_cpp", "monitor.pid")

        def _watch():
            while True:
                threading.Event().wait(5)  # 5秒ごとにチェック
                if not os.path.exists(pid_file):
                    self.root.after(0, self.root.destroy)
                    return
                try:
                    with open(pid_file) as f:
                        pid = int(f.read().strip())
                    if not _is_pid_alive(pid):
                        self.root.after(0, self.root.destroy)
                        return
                except Exception:
                    pass

        threading.Thread(target=_watch, daemon=True).start()

    def check_monitor_status(self) -> tuple[bool, str]:
        """監視サービスが稼働しているかをハートビートファイルから確認する"""
        hb_info = database.read_last_heartbeat()
        if not hb_info:
            return False, "停止中 (生存信号なし)"
        
        hb_time, _ = hb_info
        if datetime.now() - hb_time < timedelta(minutes=3):
            return True, f"稼働中 (最終更新: {hb_time.strftime('%H:%M:%S')})"
        else:
            return False, f"停止中 (最終更新: {hb_time.strftime('%m-%d %H:%M')})"

    def create_widgets(self):
        # 1. タイトル＆ステータスバー
        title_frame = tk.Frame(self.root, bg="#1e1e2e")
        title_frame.pack(fill="x", padx=25, pady=15)
        
        title_label = tk.Label(title_frame, text="睡眠トラッカー", font=("Yu Gothic UI", 22, "bold"), bg="#1e1e2e", fg="#89b4fa")
        title_label.pack(side="left")
        
        is_running, status_text = self.check_monitor_status()
        status_color = "#a6e3a1" if is_running else "#f38ba8"
        status_label = tk.Label(title_frame, text=f"監視サービス: {status_text}", font=("Yu Gothic UI", 10, "bold"), bg="#1e1e2e", fg=status_color)
        status_label.pack(side="right", pady=8)

        # 2. 上部サマリーカードエリア（予測＆統計）
        summary_frame = tk.Frame(self.root, bg="#1e1e2e")
        summary_frame.pack(fill="x", padx=25, pady=5)
        
        # 予測カード
        self.pred_card = ttk.Frame(summary_frame, style="Card.TFrame")
        self.pred_card.pack(side="left", fill="both", expand=True, padx=(0, 10))
        
        # 統計カード
        self.stats_card = ttk.Frame(summary_frame, style="Card.TFrame")
        self.stats_card.pack(side="right", fill="both", expand=True, padx=(10, 0))

        self.update_prediction_and_stats()

        # 3. ナビゲーションコントロールエリア (カレンダー＆週切り替え)
        nav_frame = tk.Frame(self.root, bg="#1e1e2e")
        nav_frame.pack(fill="x", padx=25, pady=(15, 5))

        # 前の週ボタン
        prev_btn = ttk.Button(nav_frame, text="◀ 前の週", command=self.go_to_prev_week)
        prev_btn.pack(side="left", padx=5)

        # 週の表示ラベル
        self.week_label = tk.Label(nav_frame, text="", font=("Yu Gothic UI", 13, "bold"), bg="#1e1e2e", fg="#cdd6f4")
        self.week_label.pack(side="left", expand=True)

        # 次の週ボタン
        next_btn = ttk.Button(nav_frame, text="次の週 ▶", command=self.go_to_next_week)
        next_btn.pack(side="right", padx=5)

        # 今週ボタン
        today_btn = ttk.Button(nav_frame, text="今週", command=self.go_to_today)
        today_btn.pack(side="right", padx=5)

        # カレンダー日付選択コントロール (Entry + カレンダーボタンの分離設計)
        # クッキリとした「白背景・黒文字」で視認性を最優先にします
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
        
        # カレンダー起動ボタン
        cal_btn = ttk.Button(nav_frame, text="📅", width=3, command=self.open_calendar_popup)
        cal_btn.pack(side="right", padx=5)

        # 4. グラフ表示エリア
        self.graph_frame = ttk.Frame(self.root, style="Card.TFrame")
        self.graph_frame.pack(fill="both", expand=True, padx=25, pady=(5, 25))
        
        self.canvas = None
        self.update_week_view()

    def open_calendar_popup(self):
        """カスタムポップアップカレンダーを開く"""
        try:
            current_date = datetime.strptime(self.date_var.get(), "%Y-%m-%d")
        except Exception:
            current_date = datetime.now()
            
        CustomCalendar(self.root, current_date, self.on_date_selected_from_popup)

    def on_date_selected_from_popup(self, date_str):
        """ポップアップカレンダーで日付が選択された時のコールバック"""
        self.date_var.set(date_str)
        selected_dt = datetime.strptime(date_str, "%Y-%m-%d")
        self.current_week_start = self.get_week_start_monday(selected_dt)
        self.update_week_view()

    def update_prediction_and_stats(self):
        """予測データと統計情報を更新してUIに描画する"""
        # 予測の再計算
        now = datetime.now()
        pred_duration, pred_method = analyzer.predict_sleep_duration(self.sessions, now)
        pred_wake_time = now + timedelta(hours=pred_duration)
        
        # 予測カードのテキスト更新
        for widget in self.pred_card.winfo_children():
            widget.destroy()
            
        tk.Label(self.pred_card, text="睡眠時間の予測", font=("Yu Gothic UI", 11, "bold"), bg="#252538", fg="#bac2de").pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(self.pred_card, text=f"今眠った場合の予測 ({now.strftime('%H:%M')} 入眠と仮定):", font=("Yu Gothic", 10), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15)
        
        pred_time_str = f"{int(pred_duration)}時間 {int((pred_duration % 1) * 60)}分"
        tk.Label(self.pred_card, text=pred_time_str, font=("Yu Gothic UI", 24, "bold"), bg="#252538", fg="#f9e2af").pack(anchor="w", padx=15, pady=2)
        
        method_ja = pred_method.replace("Heuristic", "簡易統計").replace("Machine Learning", "機械学習").replace("Awake Duration", "連続覚醒時間")
        tk.Label(self.pred_card, text=f"予測起床時刻: {pred_wake_time.strftime('%H:%M')}頃 ({method_ja})", font=("Yu Gothic Italic", 9), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15, pady=(0, 10))

        # 統計データの再計算
        avg_sleep = 0.0
        last_sleep = 0.0
        total_days = len(self.sessions)
        if total_days > 0:
            df = pd.DataFrame(self.sessions, columns=['start', 'end', 'dur', 'type'])
            avg_sleep = df['dur'].mean()
            last_sleep = df['dur'].iloc[-1]
            
        # 統計カードのテキスト更新
        for widget in self.stats_card.winfo_children():
            widget.destroy()
            
        tk.Label(self.stats_card, text="睡眠の統計", font=("Yu Gothic UI", 11, "bold"), bg="#252538", fg="#bac2de").pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(self.stats_card, text=f"合計記録日数: {total_days} 日", font=("Yu Gothic", 10), bg="#252538", fg="#a6adc8").pack(anchor="w", padx=15)
        
        avg_str = f"平均睡眠時間: {int(avg_sleep)}時間 {int((avg_sleep % 1) * 60)}分"
        tk.Label(self.stats_card, text=avg_str, font=("Yu Gothic UI", 15, "bold"), bg="#252538", fg="#a6e3a1").pack(anchor="w", padx=15, pady=2)
        
        last_str = f"前回の睡眠時間: {int(last_sleep)}時間 {int((last_sleep % 1) * 60)}分" if total_days > 0 else "前回の睡眠時間: 記録なし"
        tk.Label(self.stats_card, text=last_str, font=("Yu Gothic", 10), bg="#252538", fg="#cdd6f4").pack(anchor="w", padx=15, pady=(0, 10))

    def go_to_prev_week(self):
        """1週間戻る"""
        self.current_week_start -= timedelta(days=7)
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def go_to_next_week(self):
        """1週間進む"""
        self.current_week_start += timedelta(days=7)
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def go_to_today(self):
        """今日を含む週に戻る"""
        self.current_week_start = self.get_week_start_monday(datetime.now())
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def update_week_view(self):
        """現在選択されている週の月曜〜日曜の睡眠時間を再集計してグラフを描画する"""
        week_end = self.current_week_start + timedelta(days=6)
        label_text = f"{self.current_week_start.strftime('%Y/%m/%d')} (月)  〜  {week_end.strftime('%Y/%m/%d')} (日)"
        self.week_label.config(text=label_text)

        # グラフ領域の再描画
        if self.canvas:
            self.canvas.get_tk_widget().destroy()
            
        self.plot_weekly_graph()

    def plot_weekly_graph(self):
        # グラフ領域の作成 (facecolorを統一)
        fig = Figure(figsize=(7, 4), dpi=100, facecolor="#252538")
        ax = fig.add_subplot(111)
        ax.set_facecolor("#252538")
        ax.grid(True, color="#313244", linestyle="--", linewidth=0.5)
        
        # 曜日のリスト (月曜始まり)
        weekdays_ja = ['月', '火', '水', '木', '金', '土', '日']
        durations = [0.0] * 7
        
        # 各曜日（月〜日）の日付範囲
        days_in_week = [self.current_week_start + timedelta(days=i) for i in range(7)]
        
        # 曜日名に実際の日付 (月/日) を結合して改行付きラベルを作成 (例: "月\n(06/22)")
        xticklabels = [f"{w}\n({d.strftime('%m/%d')})" for w, d in zip(weekdays_ja, days_in_week)]
        
        # 選択された週に含まれる睡眠データを集計
        for start_time_str, _, dur, _ in self.sessions:
            try:
                start_dt = datetime.strptime(start_time_str, "%Y-%m-%d %H:%M:%S")
                for idx, day in enumerate(days_in_week):
                    if start_dt.date() == day.date():
                        durations[idx] += dur
                        break
            except Exception:
                continue

        # 軸と目盛りのスタイル調整 (フォントファミリを游ゴシック/メイリオに固定)
        ax.spines['bottom'].color = '#45475a'
        ax.spines['left'].color = '#45475a'
        ax.spines['top'].visible = False
        ax.spines['right'].visible = False
        
        ax.set_xticks(range(7))
        # 横軸ラベル（曜日＋日付）を設定
        ax.set_xticklabels(xticklabels, color='#bac2de', fontsize=9, fontproperties='Yu Gothic')
        
        # 縦軸の目盛りテキストの設定
        ax.tick_params(colors='#bac2de', which='both', labelsize=10)
        ax.set_ylabel("睡眠時間 (時間)", color="#bac2de", fontsize=10, fontproperties='Yu Gothic')

        # 棒グラフの描画
        has_data = any(d > 0 for d in durations)
        if has_data:
            # 棒グラフ描画 (ブルー)
            bars = ax.bar(weekdays_ja, durations, color="#89b4fa", width=0.55, edgecolor="#b4befe", linewidth=0.8)
            
            # 各曜日の棒の上に睡眠時間を表示 (0時間より大きい場合のみ)
            for bar in bars:
                height = bar.get_height()
                if height > 0:
                    ax.annotate(f'{height:.1f}h',
                                xy=(bar.get_x() + bar.get_width() / 2, height),
                                xytext=(0, 3),
                                textcoords="offset points",
                                ha='center', va='bottom', fontsize=8, color="#cdd6f4")
        else:
            # データがない場合の説明文
            ax.text(0.5, 0.5, "この週の睡眠ログデータはありません。\n(iPhoneやPCで監視サービスを実行してデータを収集してください)", 
                    ha="center", va="center", color="#a6adc8", fontsize=10, transform=ax.transAxes, fontproperties='Yu Gothic')
            ax.set_ylim(0, 10)

        # 軸の文字が見切れないように余白を自動調整する
        fig.tight_layout()
        
        # Tkinter キャンバスに統合
        self.canvas = FigureCanvasTkAgg(fig, master=self.graph_frame)
        self.canvas.draw()
        self.canvas.get_tk_widget().pack(fill="both", expand=True, padx=10, pady=(0, 10))

def _is_pid_alive(pid: int) -> bool:
    """Windows API でプロセスが生存しているか確認する"""
    import ctypes
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    STILL_ACTIVE = 259
    handle = ctypes.windll.kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return False
    exit_code = ctypes.c_ulong()
    ctypes.windll.kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code))
    ctypes.windll.kernel32.CloseHandle(handle)
    return exit_code.value == STILL_ACTIVE

def ensure_monitor_running():
    """monitor.py がバックグラウンドで動いていなければ起動する"""
    script_dir = os.path.dirname(os.path.abspath(__file__))
    base_dir = os.path.dirname(script_dir)
    pid_file = os.path.join(base_dir, "src_cpp", "monitor.pid")

    # PID ファイルでプロセスの生存を確認 (ハートビートより即時かつ正確)
    if os.path.exists(pid_file):
        try:
            with open(pid_file) as f:
                pid = int(f.read().strip())
            if _is_pid_alive(pid):
                return  # モニターは稼働中
        except Exception:
            pass

    monitor_path = os.path.join(script_dir, "monitor.py")

    # sys.executable が uv や別環境の Python の場合でも常に .venv を使う
    # (pystray 等の依存パッケージは .venv にしかインストールされていないため)
    venv_pythonw = os.path.join(base_dir, ".venv", "Scripts", "pythonw.exe")
    if os.path.exists(venv_pythonw):
        python_exe = venv_pythonw
    else:
        python_exe = sys.executable
        if not python_exe.lower().endswith("pythonw.exe"):
            candidate = os.path.join(os.path.dirname(python_exe), "pythonw.exe")
            if os.path.exists(candidate):
                python_exe = candidate

    try:
        subprocess.Popen(
            [python_exe, monitor_path],
            cwd=base_dir,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
    except Exception:
        pass

def ensure_startup_registered():
    """Windows スタートアップにモニターが登録されていなければ自動登録する"""
    startup_dir = os.path.expandvars(r'%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup')
    shortcut_path = os.path.join(startup_dir, "SleepTrackerMonitor.lnk")
    if os.path.exists(shortcut_path):
        return

    script_dir = os.path.dirname(os.path.abspath(__file__))
    base_dir = os.path.dirname(script_dir)
    monitor_path = os.path.join(script_dir, "monitor.py")
    venv_pythonw = os.path.join(base_dir, ".venv", "Scripts", "pythonw.exe")
    if not os.path.exists(venv_pythonw):
        return

    ico_path = os.path.join(script_dir, "sleep_tracker.ico")
    ico_arg = f"\nif ('{ico_path}' -ne '') {{ $Shortcut.IconLocation = '{ico_path}' }}" if os.path.exists(ico_path) else ""

    ps = f"""
$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut('{shortcut_path}')
$Shortcut.TargetPath = '{venv_pythonw}'
$Shortcut.Arguments = '"{monitor_path}"'
$Shortcut.WorkingDirectory = '{base_dir}'{ico_arg}
$Shortcut.Save()
"""
    try:
        subprocess.run(
            ["powershell", "-Command", ps],
            capture_output=True, check=True,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
    except Exception:
        pass


def main():
    ensure_startup_registered()
    ensure_monitor_running()
    root = tk.Tk()
    app = SleepTrackerApp(root)
    root.mainloop()

if __name__ == "__main__":
    main()
