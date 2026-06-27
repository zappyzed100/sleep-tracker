# File: src_python/main.py
# Description: GUI application for viewing sleep history (with calendar/weekly navigation) and predictions.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: SleepTrackerApp, main
# Dependencies: tkinter, tkcalendar, matplotlib, pandas, datetime, database, analyzer

import tkinter as tk
from tkinter import ttk
from datetime import datetime, timedelta
import os
import matplotlib
matplotlib.use("TkAgg")
# Windows用の滑らかな日本語フォント (游ゴシック, メイリオ) を最優先に設定
matplotlib.rcParams['font.family'] = ['Yu Gothic', 'Meiryo', 'MS Gothic', 'sans-serif']
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
import pandas as pd
from tkcalendar import DateEntry

import database
import analyzer

class SleepTrackerApp:
    def __init__(self, root):
        self.root = root
        self.root.title("睡眠トラッカー ＆ 予測ツール")
        self.root.geometry("950x780")
        self.root.configure(bg="#1e1e2e") # ダークモード背景

        # データベースの初期化とログの同期
        database.init_db()
        try:
            database.sync_logs_to_db()
        except Exception:
            pass # ネット未接続等によるクラッシュ防止
        
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
        
        # DateEntry (カレンダー入力フィールド) のスタイルをダークテーマに設定
        self.style.configure("DateEntry", 
                             fieldbackground="#252538", 
                             foreground="#cdd6f4", 
                             background="#313244", 
                             selectbackground="#89b4fa")
        
        self.create_widgets()

    def get_week_start_monday(self, dt: datetime) -> datetime:
        """指定された日時の週の月曜日 (00:00:00) を取得する"""
        return (dt - timedelta(days=dt.weekday())).replace(hour=0, minute=0, second=0, microsecond=0)

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

        # カレンダー日付選択
        cal_label = tk.Label(nav_frame, text="日付選択: ", font=("Yu Gothic", 10), bg="#1e1e2e", fg="#a6adc8")
        cal_label.pack(side="right", padx=(20, 5))
        
        self.date_entry = DateEntry(
            nav_frame, width=12,
            background="#313244",      # ヘッダー背景
            foreground="#cdd6f4",      # ヘッダー文字
            entrybackground="#252538", # 入力セル背景
            entryforeground="#cdd6f4", # 入力セル文字
            selectbackground="#89b4fa",# 選択日背景
            selectforeground="#1e1e2e",# 選択日文字
            normalbackground="#252538",# 通常日背景
            normalforeground="#cdd6f4",# 通常日文字
            headersbackground="#313244",
            headersforeground="#cdd6f4",
            borderwidth=2, 
            year=datetime.now().year, month=datetime.now().month, 
            day=datetime.now().day, 
            date_pattern="yyyy-mm-dd", 
            font=("Yu Gothic", 10)
        )
        self.date_entry.pack(side="right", padx=5)
        self.date_entry.bind("<<DateEntrySelected>>", self.on_date_selected)

        # 4. グラフ表示エリア
        self.graph_frame = ttk.Frame(self.root, style="Card.TFrame")
        self.graph_frame.pack(fill="both", expand=True, padx=25, pady=(5, 25))
        
        self.canvas = None
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

    def on_date_selected(self, event):
        """カレンダーから日付が選択された時のイベント"""
        selected_date = self.date_entry.get_date()
        selected_dt = datetime.combine(selected_date, datetime.min.time())
        self.current_week_start = self.get_week_start_monday(selected_dt)
        self.update_week_view()

    def go_to_prev_week(self):
        """1週間戻る"""
        self.current_week_start -= timedelta(days=7)
        self.update_date_entry_to_match_week()
        self.update_week_view()

    def go_to_next_week(self):
        """1週間進む"""
        self.current_week_start += timedelta(days=7)
        self.update_date_entry_to_match_week()
        self.update_week_view()

    def update_date_entry_to_match_week(self):
        """週が切り替わった時にカレンダーウィジェットの日付を合わせる"""
        self.date_entry.unbind("<<DateEntrySelected>>")
        self.date_entry.set_date(self.current_week_start.date())
        self.date_entry.bind("<<DateEntrySelected>>", self.on_date_selected)

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

def main():
    root = tk.Tk()
    app = SleepTrackerApp(root)
    root.mainloop()

if __name__ == "__main__":
    main()
