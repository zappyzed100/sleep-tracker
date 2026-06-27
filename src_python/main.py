# File: src_python/main.py
# Description: GUI application for viewing sleep history (with premium custom calendar popup, weekly navigation, and detail dialog) and predictions, with automatic GitHub connection warnings and lifecycle synchronization with the background monitor process.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: SleepTrackerApp, SleepSessionDetailDialog, main
# Dependencies: tkinter, messagebox, matplotlib, pandas, datetime, calendar, database, analyzer, urllib.request, threading, ctypes, lifecycle

import tkinter as tk
from tkinter import ttk, messagebox
from datetime import datetime, timedelta
import os
import sys
import ctypes
import matplotlib

matplotlib.use("TkAgg")
matplotlib.rcParams["font.family"] = ["Yu Gothic", "Meiryo", "MS Gothic", "sans-serif"]
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
import pandas as pd
import urllib.request
import json
import csv
import threading

import database
import analyzer
from calendar_ui import CustomCalendar
import lifecycle

_ICO_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico"
)
_BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _apply_taskbar_relaunch(hwnd: int):
    """タスクバーピン留め時に正しいアイコン・コマンドで再起動できるよう RelaunchCommand を設定する"""
    try:

        class GUID(ctypes.Structure):
            _fields_ = [
                ("Data1", ctypes.c_ulong),
                ("Data2", ctypes.c_ushort),
                ("Data3", ctypes.c_ushort),
                ("Data4", ctypes.c_ubyte * 8),
            ]

        class PROPERTYKEY(ctypes.Structure):
            _fields_ = [("fmtid", GUID), ("pid", ctypes.c_ulong)]

        class PROPVARIANT(ctypes.Structure):
            _fields_ = [
                ("vt", ctypes.c_ushort),
                ("r1", ctypes.c_ushort),
                ("r2", ctypes.c_ushort),
                ("r3", ctypes.c_ushort),
                ("data", ctypes.c_void_p),
            ]

        # {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3} System.AppUserModel.*
        FMTID = GUID(
            0x9F4C2855,
            0x9F79,
            0x4B39,
            (ctypes.c_ubyte * 8)(0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3),
        )
        # {886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99} IPropertyStore
        IID_IPS = GUID(
            0x886D8EEB,
            0x8CF2,
            0x4446,
            (ctypes.c_ubyte * 8)(0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99),
        )

        ps = ctypes.c_void_p()
        if (
            ctypes.windll.shell32.SHGetPropertyStoreForWindow(
                hwnd, ctypes.byref(IID_IPS), ctypes.byref(ps)
            )
            != 0
            or not ps.value
        ):
            return

        vtbl = ctypes.c_void_p.from_address(ps.value).value
        sz = ctypes.sizeof(ctypes.c_void_p)

        # vtable: 0=QI 1=AddRef 2=Release 3=GetCount 4=GetAt 5=GetValue 6=SetValue 7=Commit
        SetValue = ctypes.WINFUNCTYPE(
            ctypes.HRESULT,
            ctypes.c_void_p,
            ctypes.POINTER(PROPERTYKEY),
            ctypes.POINTER(PROPVARIANT),
        )(ctypes.c_void_p.from_address(vtbl + 6 * sz).value)
        Commit = ctypes.WINFUNCTYPE(ctypes.HRESULT, ctypes.c_void_p)(
            ctypes.c_void_p.from_address(vtbl + 7 * sz).value
        )
        Release = ctypes.WINFUNCTYPE(ctypes.HRESULT, ctypes.c_void_p)(
            ctypes.c_void_p.from_address(vtbl + 2 * sz).value
        )

        propsys = ctypes.windll.propsys

        def set_str_prop(pid: int, value: str):
            pk = PROPERTYKEY()
            pk.fmtid = FMTID
            pk.pid = pid
            pv = PROPVARIANT()
            propsys.InitPropVariantFromString(value, ctypes.byref(pv))
            SetValue(ps, ctypes.byref(pk), ctypes.byref(pv))
            propsys.PropVariantClear(ctypes.byref(pv))

        pythonw = os.path.join(_BASE_DIR, ".venv", "Scripts", "pythonw.exe")
        if not os.path.exists(pythonw):
            pythonw = sys.executable.replace("python.exe", "pythonw.exe")
        main_py = os.path.join(_BASE_DIR, "src_python", "main.py")

        set_str_prop(5, "SleepTracker.UI.1")  # AppUserModel.ID
        set_str_prop(
            2, f'"{pythonw}" "{main_py}"'
        )  # RelaunchCommand (pid=3 がないと無視される)
        set_str_prop(3, "睡眠トラッカー")  # RelaunchDisplayNameResource (必須)

        Commit(ps)
        Release(ps)
    except Exception:
        pass


def _apply_dark_titlebar(widget):
    """Windows のタイトルバーをダークモードにし、タスクバーアイコンと RelaunchCommand を設定する"""
    try:
        hwnd = ctypes.windll.user32.GetParent(widget.winfo_id())
        if not hwnd:
            hwnd = widget.winfo_id()
        # ダークタイトルバー
        value = ctypes.c_int(1)
        ctypes.windll.dwmapi.DwmSetWindowAttribute(
            hwnd, 20, ctypes.byref(value), ctypes.sizeof(value)
        )
        # タスクバー/ウィンドウアイコンを .ico から直接読み込んで設定
        if os.path.exists(_ICO_PATH):
            LR_LOADFROMFILE = 0x10
            IMAGE_ICON = 1
            WM_SETICON = 0x80
            hbig = ctypes.windll.user32.LoadImageW(
                None, _ICO_PATH, IMAGE_ICON, 32, 32, LR_LOADFROMFILE
            )
            hsml = ctypes.windll.user32.LoadImageW(
                None, _ICO_PATH, IMAGE_ICON, 16, 16, LR_LOADFROMFILE
            )
            ctypes.windll.user32.SendMessageW(hwnd, WM_SETICON, 1, hbig)  # ICON_BIG
            ctypes.windll.user32.SendMessageW(hwnd, WM_SETICON, 0, hsml)  # ICON_SMALL
        # タスクバーピン留め用 RelaunchCommand / Icon を設定
        _apply_taskbar_relaunch(hwnd)
    except Exception:
        pass


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
            ico_path = os.path.join(
                os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico"
            )
            if os.path.exists(ico_path):
                self.iconbitmap(ico_path)
        except Exception:
            pass

        self.create_widgets()
        self.refresh_session_list()
        self.after(50, lambda: _apply_dark_titlebar(self))

    def create_widgets(self):
        # 1. タイトルヘッダー
        date_str = self.target_date.strftime("%Y年 %m月 %d日")
        weekday_str = ["月", "火", "水", "木", "金", "土", "日"][
            self.target_date.weekday()
        ]
        header = tk.Label(
            self,
            text=f"🌙 {date_str} ({weekday_str}) の睡眠詳細",
            font=("Yu Gothic UI", 14, "bold"),
            bg="#1e1e2e",
            fg="#f9e2af",
        )
        header.pack(fill="x", padx=20, pady=15)

        # 2. セッション一覧エリア
        list_label = tk.Label(
            self,
            text="睡眠セッション一覧 (開始時間基準):",
            font=("Yu Gothic UI", 10, "bold"),
            bg="#1e1e2e",
            fg="#bac2de",
        )
        list_label.pack(anchor="w", padx=20, pady=(5, 2))

        self.list_frame = tk.Frame(self, bg="#252538", bd=1, relief="solid")
        self.list_frame.pack(fill="both", expand=True, padx=20, pady=5)

        # 3. 手動追加エリア
        add_frame = tk.LabelFrame(
            self,
            text="睡眠記録を手動で追加",
            font=("Yu Gothic UI", 10, "bold"),
            bg="#1e1e2e",
            fg="#a6e3a1",
            bd=1,
            labelanchor="n",
        )
        add_frame.pack(fill="x", padx=20, pady=(15, 20))

        grid = tk.Frame(add_frame, bg="#1e1e2e")
        grid.pack(padx=15, pady=10)

        # --- 入眠時間 ---
        tk.Label(grid, text="入眠 (寝たとき):", bg="#1e1e2e", fg="#cdd6f4").grid(
            row=0, column=0, sticky="w", pady=5
        )
        self.start_date_var = tk.StringVar(value=self.target_date.strftime("%Y-%m-%d"))
        start_date_entry = tk.Entry(
            grid,
            textvariable=self.start_date_var,
            width=12,
            state="readonly",
            font=("Yu Gothic UI", 9, "bold"),
            bg="white",
            fg="black",
            readonlybackground="white",
        )
        start_date_entry.grid(row=0, column=1, padx=5)

        start_cal_btn = ttk.Button(
            grid,
            text="📅",
            width=3,
            command=lambda: self.open_calendar_for(self.start_date_var),
        )
        start_cal_btn.grid(row=0, column=2, padx=2)

        self.start_hour = ttk.Combobox(
            grid, values=[f"{i:02d}" for i in range(24)], width=4, state="readonly"
        )
        self.start_hour.set("23")
        self.start_hour.grid(row=0, column=3, padx=2)
        tk.Label(grid, text="時", bg="#1e1e2e", fg="#cdd6f4").grid(row=0, column=4)

        self.start_min = ttk.Combobox(
            grid, values=[f"{i:02d}" for i in range(60)], width=4, state="readonly"
        )
        self.start_min.set("00")
        self.start_min.grid(row=0, column=5, padx=2)
        tk.Label(grid, text="分", bg="#1e1e2e", fg="#cdd6f4").grid(row=0, column=6)

        # --- 起床時間 ---
        tk.Label(grid, text="起床 (起きたとき):", bg="#1e1e2e", fg="#cdd6f4").grid(
            row=1, column=0, sticky="w", pady=5
        )
        self.end_date_var = tk.StringVar(
            value=(self.target_date + timedelta(days=1)).strftime("%Y-%m-%d")
        )
        end_date_entry = tk.Entry(
            grid,
            textvariable=self.end_date_var,
            width=12,
            state="readonly",
            font=("Yu Gothic UI", 9, "bold"),
            bg="white",
            fg="black",
            readonlybackground="white",
        )
        end_date_entry.grid(row=1, column=1, padx=5)

        end_cal_btn = ttk.Button(
            grid,
            text="📅",
            width=3,
            command=lambda: self.open_calendar_for(self.end_date_var),
        )
        end_cal_btn.grid(row=1, column=2, padx=2)

        self.end_hour = ttk.Combobox(
            grid, values=[f"{i:02d}" for i in range(24)], width=4, state="readonly"
        )
        self.end_hour.set("07")
        self.end_hour.grid(row=1, column=3, padx=2)
        tk.Label(grid, text="時", bg="#1e1e2e", fg="#cdd6f4").grid(row=1, column=4)

        self.end_min = ttk.Combobox(
            grid, values=[f"{i:02d}" for i in range(60)], width=4, state="readonly"
        )
        self.end_min.set("00")
        self.end_min.grid(row=1, column=5, padx=2)
        tk.Label(grid, text="分", bg="#1e1e2e", fg="#cdd6f4").grid(row=1, column=6)

        add_btn = ttk.Button(
            add_frame, text="この睡眠データを手動追加する", command=self.add_session
        )
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
        target_str = self.target_date.strftime("%Y-%m-%d")
        day_sessions = []
        for s_id, s_time, e_time, dur, s_type in sessions:
            if s_time.startswith(target_str):
                day_sessions.append((s_id, s_time, e_time, dur, s_type))

        if not day_sessions:
            lbl = tk.Label(
                self.list_frame,
                text="この日の睡眠記録はありません。",
                font=("Yu Gothic", 10),
                bg="#252538",
                fg="#a6adc8",
            )
            lbl.pack(expand=True, fill="both", pady=30)
            return

        for s_id, s_time, e_time, dur, s_type in day_sessions:
            item_frame = tk.Frame(self.list_frame, bg="#252538")
            item_frame.pack(fill="x", padx=10, pady=5)

            st_time = s_time.split(" ")[1][:5]
            ed_time = e_time.split(" ")[1][:5] if e_time else "--:--"
            h = int(dur)
            m = int((dur % 1) * 60)
            type_lbl = " [外出]" if s_type == "out" else ""

            text_str = f"⏰ {st_time} 〜 {ed_time} ({h}時間{m}分){type_lbl}"
            lbl = tk.Label(
                item_frame,
                text=text_str,
                font=("Yu Gothic UI", 10, "bold"),
                bg="#252538",
                fg="#cdd6f4",
            )
            lbl.pack(side="left", padx=5)

            del_btn = tk.Button(
                item_frame,
                text="削除",
                font=("Yu Gothic UI", 9, "bold"),
                bg="#f38ba8",
                fg="#11111b",
                activebackground="#eba0b2",
                activeforeground="#11111b",
                bd=0,
                padx=8,
                pady=2,
                cursor="hand2",
                command=lambda idx=s_id: self.delete_session(idx),
            )
            del_btn.pack(side="right", padx=5)

    def delete_session(self, session_id):
        if messagebox.askyesno(
            "削除確認",
            "この睡眠データを完全に削除しますか？\n( sleep_events.txt を再構成し、GitHubへプッシュします)",
            icon="warning",
        ):
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

        success, message = database.add_session_and_rebuild(
            start_datetime, end_datetime, "sleep"
        )
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
            ico_path = os.path.join(
                os.path.dirname(os.path.abspath(__file__)), "sleep_tracker.ico"
            )
            if os.path.exists(ico_path):
                self.root.iconbitmap(ico_path)
        except Exception:
            pass

        self.stats_period = "1w"  # 平均睡眠時間の集計期間: 1w / 1m / 1y / all

        # 1. UI起動時にモニターが動いていなければ自動起動する (ライフサイクル同期)
        lifecycle.ensure_monitor_running()

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
        self.style.configure(
            "TLabel", background="#1e1e2e", foreground="#cdd6f4", font=("Yu Gothic", 11)
        )
        self.style.configure("Card.TFrame", background="#252538", relief="flat")
        self.style.configure(
            "TButton",
            font=("Yu Gothic UI", 10, "bold"),
            background="#313244",
            foreground="#cdd6f4",
        )
        self.style.map(
            "TButton",
            background=[("active", "#45475a"), ("pressed", "#585b70")],
            foreground=[("active", "#cdd6f4")],
        )
        # 手動追加ダイアログ内コンボボックスの文字を黒にする
        self.style.configure(
            "TCombobox",
            foreground="black",
            fieldbackground="white",
            selectforeground="black",
            selectbackground="#c5e8ff",
        )
        self.style.map(
            "TCombobox",
            fieldbackground=[("readonly", "white")],
            foreground=[("readonly", "black")],
            selectforeground=[("readonly", "black")],
            selectbackground=[("readonly", "#c5e8ff")],
        )

        self.create_widgets()
        self.root.after(50, lambda: _apply_dark_titlebar(self.root))
        self.periodic_connection_check()

        # config.json がなければ初回セットアップとして設定タブを自動で開く
        if not os.path.exists(database.CONFIG_PATH):
            self.switch_tab("settings")

        # 2. モニター終了を即時検知 (WaitForSingleObject) + 30s フォールバック
        self._watch_monitor_exit()
        self.root.after(30000, self.monitor_lifecycle_check)

    def get_week_start_monday(self, dt: datetime) -> datetime:
        return (dt - timedelta(days=dt.weekday())).replace(
            hour=0, minute=0, second=0, microsecond=0
        )

    def toggle_startup(self):
        if self.startup_var.get():
            lifecycle.ensure_startup_registered()
        else:
            lifecycle.remove_startup_registration()

    def _load_threshold(self) -> int:
        config_path = os.path.join(
            os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "config.json"
        )
        try:
            with open(config_path, "r", encoding="utf-8") as f:
                return int(json.load(f).get("idle_threshold_minutes", 20))
        except Exception:
            return 20

    def save_threshold(self):
        try:
            minutes = max(1, int(self.threshold_var.get()))
            self.threshold_var.set(str(minutes))
            config_path = os.path.join(
                os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                "config.json",
            )
            with open(config_path, "r", encoding="utf-8") as f:
                config = json.load(f)
            config["idle_threshold_minutes"] = minutes
            with open(config_path, "w", encoding="utf-8") as f:
                json.dump(config, f, indent=4, ensure_ascii=False)
        except Exception:
            pass

    def _set_stats_period(self, period: str):
        self.stats_period = period
        self.update_prediction_and_stats()

    def export_csv(self):
        from tkinter import filedialog

        file_path = filedialog.asksaveasfilename(
            defaultextension=".csv",
            filetypes=[("CSV ファイル", "*.csv")],
            initialfile=f"sleep_data_{datetime.now().strftime('%Y%m%d')}.csv",
            title="睡眠データをCSVで保存",
        )
        if not file_path:
            return
        try:
            sessions = database.get_all_sessions()
            with open(file_path, "w", newline="", encoding="utf-8-sig") as f:
                writer = csv.writer(f)
                writer.writerow(["就寝時刻", "起床時刻", "睡眠時間(時間)", "種別"])
                for start, end, dur, stype in sessions:
                    writer.writerow([start, end or "", f"{dur:.2f}", stype])
            messagebox.showinfo("エクスポート完了", f"CSVを保存しました:\n{file_path}")
        except Exception as e:
            messagebox.showerror("エクスポートエラー", str(e))

    def import_csv(self):
        from tkinter import filedialog

        file_path = filedialog.askopenfilename(
            filetypes=[("CSV ファイル", "*.csv"), ("すべてのファイル", "*.*")],
            title="インポートするCSVファイルを選択",
        )
        if not file_path:
            return
        try:
            sessions = []
            with open(file_path, "r", encoding="utf-8-sig") as f:
                reader = csv.DictReader(f)
                headers = reader.fieldnames or []
                # 列名を柔軟に検出 (日本語/英語どちらでも対応)
                col_start = next(
                    (h for h in headers if "就寝" in h or "start" in h.lower()),
                    headers[0] if headers else None,
                )
                col_end = next(
                    (h for h in headers if "起床" in h or "end" in h.lower()),
                    headers[1] if len(headers) > 1 else None,
                )
                col_type = next(
                    (h for h in headers if "種別" in h or "type" in h.lower()), None
                )
                if col_start is None or col_end is None:
                    messagebox.showerror(
                        "列検出エラー",
                        "就寝時刻・起床時刻の列が見つかりませんでした。\n列名を確認してください。",
                    )
                    return
                for row in reader:
                    start = row.get(col_start, "").strip()
                    end = row.get(col_end, "").strip()
                    stype = row.get(col_type, "sleep").strip() if col_type else "sleep"
                    if start and end:
                        sessions.append((start, end, stype or "sleep"))
            if not sessions:
                messagebox.showwarning(
                    "データなし", "インポートできる行がありませんでした。"
                )
                return

            def run():
                imported, skipped = database.bulk_import_sessions(sessions)
                msg = f"インポート完了\n\n追加: {imported} 件\nスキップ (重複 / 不正): {skipped} 件"
                self.root.after(0, self.update_week_view)
                self.root.after(0, lambda: messagebox.showinfo("インポート完了", msg))

            threading.Thread(target=run, daemon=True).start()
        except Exception as e:
            messagebox.showerror(
                "インポートエラー", f"CSVの読み込みに失敗しました:\n{e}"
            )

    def force_sync_ui(self):
        def run():
            try:
                database.validate_gist_connection()
                database.sync_logs_to_db()
                self.root.after(0, self.hide_connection_warning)
                self.root.after(
                    0,
                    lambda: messagebox.showinfo(
                        "同期完了", "Gistとの同期が完了しました。"
                    ),
                )
            except Exception as e:
                err_msg = str(e)
                self.root.after(0, lambda: messagebox.showerror("同期エラー", err_msg))

        threading.Thread(target=run, daemon=True).start()

    def clear_all_data_ui(self):
        if not messagebox.askyesno(
            "全データ削除の確認",
            "本当にすべての睡眠データを削除しますか？\n\nこの操作は取り消せません。",
            icon="warning",
        ):
            return

        def run():
            try:
                database.clear_all_data()
                database.sync_logs_to_db()
                self.root.after(0, self.update_week_view)
                self.root.after(
                    0,
                    lambda: messagebox.showinfo(
                        "削除完了", "すべてのデータを削除しました。"
                    ),
                )
            except Exception as e:
                err_msg = str(e)
                self.root.after(0, lambda: messagebox.showerror("削除エラー", err_msg))

        threading.Thread(target=run, daemon=True).start()

    def create_desktop_shortcut_ui(self):
        if lifecycle.create_desktop_shortcut():
            messagebox.showinfo("完了", "デスクトップにショートカットを作成しました。")
        else:
            messagebox.showerror("失敗", "ショートカットの作成に失敗しました。")

    def save_github_config(self):
        token = self.token_var.get().strip()
        gist_id = self.gist_id_var.get().strip()
        try:
            config = {}
            if os.path.exists(database.CONFIG_PATH):
                with open(database.CONFIG_PATH, "r", encoding="utf-8") as f:
                    config = json.load(f)
            config["github_token"] = token
            config["gist_id"] = gist_id
            with open(database.CONFIG_PATH, "w", encoding="utf-8") as f:
                json.dump(config, f, indent=4, ensure_ascii=False)
            self.github_status_label.config(text="設定を保存しました", fg="#a6e3a1")
        except Exception as e:
            self.github_status_label.config(text=f"✗ 保存失敗: {e}", fg="#f38ba8")

    def test_github_connection(self):
        self.github_status_label.config(text="接続確認中...", fg="#f9e2af")

        def run():
            gist_id = self.gist_id_var.get().strip()
            token = self.token_var.get().strip()
            if not gist_id or not token:
                self.root.after(
                    0,
                    lambda: self.github_status_label.config(
                        text="✗ Gist IDまたはトークンが未入力です", fg="#f38ba8"
                    ),
                )
                return
            url = f"https://api.github.com/gists/{gist_id}"
            headers = {
                "Authorization": f"Bearer {token}",
                "Accept": "application/vnd.github+json",
                "User-Agent": "Sleep-Tracker-Client",
            }
            req = urllib.request.Request(url, headers=headers)
            try:
                with urllib.request.urlopen(req, timeout=8) as _:
                    now_str = datetime.now().strftime("%H:%M")
                    self.root.after(
                        0,
                        lambda: self.github_status_label.config(
                            text=f"✓ 接続成功（確認時刻: {now_str}）", fg="#a6e3a1"
                        ),
                    )
            except Exception as e:
                err = str(e)
                if "401" in err:
                    reason = "401 Unauthorized — トークンが無効です"
                elif "404" in err:
                    reason = "404 Not Found — Gist IDが無効です"
                else:
                    reason = "接続タイムアウト / オフライン"
                self.root.after(
                    0,
                    lambda: self.github_status_label.config(
                        text=f"✗ {reason}", fg="#f38ba8"
                    ),
                )

        threading.Thread(target=run, daemon=True).start()

    def toggle_out_state(self):
        """検知中断 / 再開をトグルして sleep_events.txt に記録する"""
        event = "OUT_END" if self._is_out else "OUT_START"
        database.record_out_event(event)
        self._is_out = not self._is_out
        self._update_out_ui()

    def _update_out_ui(self):
        if self._is_out:
            self.out_status_label.config(text="検知中断中", fg="#f9e2af")
            self.out_toggle_btn.config(
                text="再開する", bg="#45475a", fg="#cdd6f4",
                activebackground="#585b70", activeforeground="#cdd6f4",
            )
        else:
            self.out_status_label.config(text="検知中", fg="#a6e3a1")
            self.out_toggle_btn.config(
                text="中断する", bg="#45475a", fg="#cdd6f4",
                activebackground="#585b70", activeforeground="#cdd6f4",
            )

    def _watch_monitor_exit(self):
        """モニタープロセスの終了を WaitForSingleObject で即時検知し、UI を閉じる"""

        def watch():
            import time

            for _ in range(20):
                if os.path.exists(lifecycle.PID_FILE):
                    break
                time.sleep(0.3)
            try:
                with open(lifecycle.PID_FILE, "r") as f:
                    pid = int(f.read().strip())
            except Exception:
                return
            k32 = ctypes.windll.kernel32
            k32.OpenProcess.restype = ctypes.c_void_p
            handle = k32.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
            if not handle:
                return
            k32.WaitForSingleObject(handle, 0xFFFFFFFF)  # INFINITE 待機
            k32.CloseHandle(handle)
            try:
                self.root.after(0, self.root.destroy)
            except Exception:
                pass

        threading.Thread(target=watch, daemon=True).start()

    def monitor_lifecycle_check(self):
        """モニターの生存を確認し、切れている（トレイから終了された）場合はUIも切る"""
        pid_file = lifecycle.PID_FILE

        # 1. PIDファイルが削除されている場合
        if not os.path.exists(pid_file):
            print("Monitor PID file removed. Shutting down UI.")
            self.root.destroy()
            return

        # 2. プロセスリストを確認して PID の存在確認
        try:
            with open(pid_file, "r") as f:
                pid = int(f.read().strip())

            if not lifecycle.check_process_exists(pid):
                print("Monitor process was killed. Shutting down UI.")
                self.root.destroy()
                return
        except Exception:
            # 取得失敗時はハートビート時間でフォールバック確認
            hb_info = lifecycle.read_last_heartbeat()
            if hb_info:
                hb_time, _ = hb_info
                if datetime.now() - hb_time > timedelta(minutes=3):
                    print("Stale heartbeat detected. Shutting down UI.")
                    self.root.destroy()
                    return

        self.root.after(30000, self.monitor_lifecycle_check)

    def create_widgets(self):
        # ── タイトル行 ───────────────────────────────────────────
        title_frame = tk.Frame(self.root, bg="#1e1e2e")
        title_frame.pack(fill="x", padx=25, pady=(15, 5))

        tk.Label(
            title_frame,
            text="睡眠トラッカー",
            font=("Yu Gothic UI", 22, "bold"),
            bg="#1e1e2e",
            fg="#89b4fa",
        ).pack(side="left")

        is_running, status_text = lifecycle.is_monitor_running()
        status_color = "#a6e3a1" if is_running else "#f38ba8"
        tk.Label(
            title_frame,
            text=f"バックグラウンドサービス: {status_text}",
            font=("Yu Gothic UI", 10, "bold"),
            bg="#1e1e2e",
            fg=status_color,
        ).pack(side="right", pady=8)

        # ── 検知中断トグル ─────────────────────────────────────────
        self._is_out = database.get_current_out_state()
        out_frame = tk.Frame(title_frame, bg="#1e1e2e")
        out_frame.pack(side="right", padx=(0, 20))
        self.out_status_label = tk.Label(
            out_frame, text="", font=("Yu Gothic UI", 14, "bold"), bg="#1e1e2e"
        )
        self.out_status_label.pack(side="left", padx=(0, 8))
        self.out_toggle_btn = tk.Button(
            out_frame,
            text="",
            font=("Yu Gothic UI", 9),
            bd=0,
            padx=8,
            pady=2,
            cursor="hand2",
            relief="flat",
            command=self.toggle_out_state,
        )
        self.out_toggle_btn.pack(side="left")
        self._update_out_ui()

        # ── タブバー ──────────────────────────────────────────────
        tab_bar = tk.Frame(self.root, bg="#1e1e2e")
        tab_bar.pack(fill="x", padx=25, pady=(0, 0))
        self.tab_btns = {}
        for name, label in [("home", "ホーム"), ("settings", "設定")]:
            btn = tk.Button(
                tab_bar,
                text=label,
                font=("Yu Gothic UI", 10, "bold"),
                bg="#313244",
                fg="#cdd6f4",
                activebackground="#313244",
                activeforeground="#cdd6f4",
                bd=0,
                padx=14,
                pady=6,
                cursor="hand2",
                command=lambda t=name: self.switch_tab(t),
            )
            btn.pack(side="left")
            self.tab_btns[name] = btn
        self.switch_tab("home", init=True)  # 初期スタイル設定のみ

        ttk.Separator(self.root, orient="horizontal").pack(fill="x")

        # ── ホームタブ本体 ────────────────────────────────────────
        self.home_content = tk.Frame(self.root, bg="#1e1e2e")
        self.home_content.pack(fill="both", expand=True)

        self.warning_frame = tk.Frame(
            self.home_content, bg="#f38ba8", bd=1, relief="solid"
        )
        self.warning_label = tk.Label(
            self.warning_frame,
            text="",
            font=("Yu Gothic UI", 10, "bold"),
            bg="#f38ba8",
            fg="#11111b",
        )
        self.warning_label.pack(fill="x", padx=15, pady=6)

        self.summary_frame = tk.Frame(self.home_content, bg="#1e1e2e")
        self.summary_frame.pack(fill="x", padx=25, pady=5)

        self.pred_card = ttk.Frame(self.summary_frame, style="Card.TFrame")
        self.pred_card.pack(side="left", fill="both", expand=True, padx=(0, 10))

        self.stats_card = ttk.Frame(self.summary_frame, style="Card.TFrame")
        self.stats_card.pack(side="right", fill="both", expand=True, padx=(10, 0))

        self.update_prediction_and_stats()

        nav_frame = tk.Frame(self.home_content, bg="#1e1e2e")
        nav_frame.pack(fill="x", padx=25, pady=(15, 5))

        ttk.Button(nav_frame, text="◀ 前の週", command=self.go_to_prev_week).pack(
            side="left", padx=5
        )

        self.week_label = tk.Label(
            nav_frame,
            text="",
            font=("Yu Gothic UI", 13, "bold"),
            bg="#1e1e2e",
            fg="#cdd6f4",
        )
        self.week_label.pack(side="left", expand=True)

        ttk.Button(nav_frame, text="次の週 ▶", command=self.go_to_next_week).pack(
            side="right", padx=5
        )
        ttk.Button(nav_frame, text="今週", command=self.go_to_this_week).pack(
            side="right", padx=5
        )

        tk.Label(
            nav_frame,
            text="日付選択: ",
            font=("Yu Gothic", 10),
            bg="#1e1e2e",
            fg="#a6adc8",
        ).pack(side="right", padx=(10, 2))

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
            state="readonly",
        )
        self.date_entry.pack(side="right", padx=2)
        ttk.Button(
            nav_frame, text="📅", width=3, command=self.open_calendar_popup
        ).pack(side="right", padx=5)

        self.graph_frame = ttk.Frame(self.home_content, style="Card.TFrame")
        self.graph_frame.pack(fill="both", expand=True, padx=25, pady=(5, 25))

        self.canvas = None
        self.update_week_view()

        # ── 設定タブ本体（初期非表示、スクロール対応）────────────────────────────
        self.settings_wrapper = tk.Frame(self.root, bg="#1e1e2e")
        _sc = tk.Canvas(self.settings_wrapper, bg="#1e1e2e", highlightthickness=0)
        _sb = ttk.Scrollbar(self.settings_wrapper, orient="vertical", command=_sc.yview)
        _sc.configure(yscrollcommand=_sb.set)
        _sb.pack(side="right", fill="y")
        _sc.pack(side="left", fill="both", expand=True)
        self.settings_content = tk.Frame(_sc, bg="#1e1e2e")
        _sw = _sc.create_window((0, 0), window=self.settings_content, anchor="nw")
        self.settings_content.bind(
            "<Configure>", lambda e: _sc.configure(scrollregion=_sc.bbox("all"))
        )
        _sc.bind("<Configure>", lambda e: _sc.itemconfig(_sw, width=e.width))
        _sc.bind(
            "<Enter>",
            lambda e: self.root.bind_all(
                "<MouseWheel>",
                lambda ev: _sc.yview_scroll(int(-1 * (ev.delta / 120)), "units"),
            ),
        )
        _sc.bind("<Leave>", lambda e: self.root.unbind_all("<MouseWheel>"))
        self._build_settings_tab()

    def _build_settings_tab(self):
        """設定タブの中身を構築する"""
        _card = {"bg": "#252538", "bd": 1, "relief": "solid"}
        _lbl_h = {
            "font": ("Yu Gothic UI", 11, "bold"),
            "bg": "#252538",
            "fg": "#bac2de",
        }
        _lbl_b = {"font": ("Yu Gothic UI", 10), "bg": "#252538", "fg": "#cdd6f4"}
        _lbl_s = {"font": ("Yu Gothic UI", 9), "bg": "#252538", "fg": "#6c7086"}
        _kw = {"padx": 40, "fill": "x"}

        # 起動設定
        s1 = tk.Frame(self.settings_content, **_card)
        s1.pack(**_kw, pady=(30, 8))
        tk.Label(s1, text="起動設定", **_lbl_h).pack(anchor="w", padx=15, pady=(10, 4))
        self.startup_var = tk.BooleanVar(
            value=os.path.exists(lifecycle.STARTUP_SHORTCUT_PATH)
        )
        tk.Checkbutton(
            s1,
            text="PC起動時に自動でモニターを起動する",
            variable=self.startup_var,
            command=self.toggle_startup,
            bg="#252538",
            fg="#cdd6f4",
            selectcolor="#313244",
            activebackground="#252538",
            activeforeground="#cdd6f4",
            font=("Yu Gothic UI", 10),
        ).pack(anchor="w", padx=15, pady=(0, 6))
        tk.Button(
            s1,
            text="デスクトップにショートカットを作成",
            command=self.create_desktop_shortcut_ui,
            bg="#313244",
            fg="#cdd6f4",
            activebackground="#45475a",
            activeforeground="#cdd6f4",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=12,
            pady=5,
            cursor="hand2",
        ).pack(anchor="w", padx=15, pady=(0, 12))

        # 睡眠判定時間
        s2 = tk.Frame(self.settings_content, **_card)
        s2.pack(**_kw, pady=8)
        tk.Label(s2, text="睡眠判定時間", **_lbl_h).pack(
            anchor="w", padx=15, pady=(10, 4)
        )
        row = tk.Frame(s2, bg="#252538")
        row.pack(anchor="w", padx=15, pady=(0, 4))
        tk.Label(row, text="キーボード/マウス操作がない状態が", **_lbl_b).pack(
            side="left"
        )
        self.threshold_var = tk.StringVar(value=str(self._load_threshold()))
        spin = tk.Spinbox(
            row,
            from_=5,
            to=120,
            increment=5,
            textvariable=self.threshold_var,
            width=4,
            bg="white",
            fg="black",
            font=("Yu Gothic UI", 10),
            command=self.save_threshold,
        )
        spin.pack(side="left", padx=5)
        spin.bind("<Return>", lambda e: self.save_threshold())
        spin.bind("<FocusOut>", lambda e: self.save_threshold())
        tk.Label(row, text="分以上続いたら睡眠と判定", **_lbl_b).pack(side="left")
        tk.Label(s2, text="※ モニターを再起動すると反映されます", **_lbl_s).pack(
            anchor="w", padx=15, pady=(0, 12)
        )

        # CSVインポート / エクスポート
        s3 = tk.Frame(self.settings_content, **_card)
        s3.pack(**_kw, pady=8)
        tk.Label(s3, text="データのインポート / エクスポート", **_lbl_h).pack(
            anchor="w", padx=15, pady=(10, 4)
        )

        csv_grid = tk.Frame(s3, bg="#252538")
        csv_grid.pack(anchor="w", padx=15, pady=(0, 4))

        tk.Label(csv_grid, text="インポート:", **_lbl_b).grid(row=0, column=0, sticky="w", pady=(0, 6))
        tk.Button(
            csv_grid,
            text="CSVから読み込む",
            command=self.import_csv,
            bg="#a6e3a1",
            fg="#1e1e2e",
            activebackground="#94d18f",
            activeforeground="#1e1e2e",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=10,
            pady=4,
            cursor="hand2",
        ).grid(row=0, column=1, sticky="w", padx=(12, 0), pady=(0, 6))

        tk.Label(csv_grid, text="エクスポート:", **_lbl_b).grid(row=1, column=0, sticky="w")
        tk.Button(
            csv_grid,
            text="CSVに書き出す",
            command=self.export_csv,
            bg="#89b4fa",
            fg="#1e1e2e",
            activebackground="#7ba5f0",
            activeforeground="#1e1e2e",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=10,
            pady=4,
            cursor="hand2",
        ).grid(row=1, column=1, sticky="w", padx=(12, 0))

        tk.Label(
            s3,
            text="※ 列順: 就寝時刻, 起床時刻, 睡眠時間(時間), 種別 — このアプリのCSV出力と同じ形式",
            **_lbl_s,
        ).pack(anchor="w", padx=15, pady=(2, 12))

        # GitHub連携
        s4 = tk.Frame(self.settings_content, **_card)
        s4.pack(**_kw, pady=8)
        tk.Label(s4, text="GitHub連携", **_lbl_h).pack(
            anchor="w", padx=15, pady=(10, 4)
        )

        tk.Label(s4, text="Gist ID:", **_lbl_b).pack(anchor="w", padx=15, pady=(0, 2))
        self.gist_id_var = tk.StringVar()
        tk.Entry(
            s4,
            textvariable=self.gist_id_var,
            width=52,
            bg="#313244",
            fg="#cdd6f4",
            insertbackground="#cdd6f4",
            font=("Yu Gothic UI", 10),
            bd=1,
            relief="solid",
        ).pack(anchor="w", padx=15, pady=(0, 6))

        tk.Label(
            s4,
            text="Personal Access Token (Gist の read/write 権限のみでOK):",
            **_lbl_b,
        ).pack(anchor="w", padx=15, pady=(0, 2))
        self.token_var = tk.StringVar()
        tk.Entry(
            s4,
            textvariable=self.token_var,
            show="*",
            width=52,
            bg="#313244",
            fg="#cdd6f4",
            insertbackground="#cdd6f4",
            font=("Yu Gothic UI", 10),
            bd=1,
            relief="solid",
        ).pack(anchor="w", padx=15, pady=(0, 8))

        gh_btn_row = tk.Frame(s4, bg="#252538")
        gh_btn_row.pack(anchor="w", padx=15, pady=(0, 6))
        tk.Button(
            gh_btn_row,
            text="保存",
            command=self.save_github_config,
            bg="#89b4fa",
            fg="#1e1e2e",
            activebackground="#7ba5f0",
            activeforeground="#1e1e2e",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=20,
            pady=6,
            width=10,
            cursor="hand2",
        ).pack(side="left", padx=(0, 10))
        tk.Button(
            gh_btn_row,
            text="接続テスト",
            command=self.test_github_connection,
            bg="#313244",
            fg="#cdd6f4",
            activebackground="#45475a",
            activeforeground="#cdd6f4",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=20,
            pady=6,
            width=10,
            cursor="hand2",
        ).pack(side="left")

        self.github_status_label = tk.Label(
            s4, text="", font=("Yu Gothic UI", 10), bg="#252538", fg="#a6adc8"
        )
        self.github_status_label.pack(anchor="w", padx=15, pady=(4, 4))
        tk.Label(
            s4, text="※ config.json にローカル保存されます（gitignore済み）", **_lbl_s
        ).pack(anchor="w", padx=15, pady=(0, 12))

        # データ管理
        s5 = tk.Frame(self.settings_content, **_card)
        s5.pack(**_kw, pady=(8, 30))
        tk.Label(s5, text="データ管理", **_lbl_h).pack(
            anchor="w", padx=15, pady=(10, 4)
        )
        tk.Label(
            s5,
            text="sleep_events.txt を Gist にバックアップ / Gist からデータを復元します",
            **_lbl_s,
        ).pack(anchor="w", padx=15, pady=(0, 6))
        btn_row = tk.Frame(s5, bg="#252538")
        btn_row.pack(anchor="w", padx=15, pady=(0, 12))
        tk.Button(
            btn_row,
            text="今すぐ Gist と同期",
            command=self.force_sync_ui,
            bg="#313244",
            fg="#cdd6f4",
            activebackground="#45475a",
            activeforeground="#cdd6f4",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=12,
            pady=6,
            cursor="hand2",
        ).pack(side="left", padx=(0, 10))
        tk.Button(
            btn_row,
            text="全データを削除",
            command=self.clear_all_data_ui,
            bg="#f38ba8",
            fg="#11111b",
            activebackground="#eba0b2",
            activeforeground="#11111b",
            font=("Yu Gothic UI", 10, "bold"),
            bd=0,
            padx=12,
            pady=6,
            cursor="hand2",
        ).pack(side="left")

        # 既存の設定値を入力欄に反映
        try:
            with open(database.CONFIG_PATH, "r", encoding="utf-8") as f:
                _cfg = json.load(f)
            self.token_var.set(_cfg.get("github_token", ""))
            self.gist_id_var.set(_cfg.get("gist_id", ""))
        except Exception:
            pass

    def switch_tab(self, tab_name: str, init: bool = False):
        for name, btn in self.tab_btns.items():
            active = name == tab_name
            btn.config(
                bg="#313244" if active else "#1e1e2e",
                fg="#cdd6f4" if active else "#6c7086",
            )
        if init:
            return
        if tab_name == "home":
            self.settings_wrapper.pack_forget()
            self.home_content.pack(fill="both", expand=True)
        else:
            self.home_content.pack_forget()
            self.settings_wrapper.pack(fill="both", expand=True)

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
        self.sessions = database.get_all_sessions()
        now = datetime.now()
        pred_duration, pred_method = analyzer.predict_sleep_duration(self.sessions, now)
        pred_wake_time = now + timedelta(hours=pred_duration)

        for widget in self.pred_card.winfo_children():
            widget.destroy()

        tk.Label(
            self.pred_card,
            text="睡眠時間の予測",
            font=("Yu Gothic UI", 11, "bold"),
            bg="#252538",
            fg="#bac2de",
        ).pack(anchor="w", padx=15, pady=(10, 2))
        tk.Label(
            self.pred_card,
            text=f"今眠った場合の予測 ({now.strftime('%H:%M')} 入眠と仮定):",
            font=("Yu Gothic", 10),
            bg="#252538",
            fg="#a6adc8",
        ).pack(anchor="w", padx=15)

        pred_time_str = f"{int(pred_duration)}時間 {int((pred_duration % 1) * 60)}分"
        tk.Label(
            self.pred_card,
            text=pred_time_str,
            font=("Yu Gothic UI", 24, "bold"),
            bg="#252538",
            fg="#f9e2af",
        ).pack(anchor="w", padx=15, pady=2)

        method_ja = (
            pred_method.replace("Heuristic", "簡易統計")
            .replace("Machine Learning", "機械学習")
            .replace("Awake Duration", "連続覚醒時間")
        )
        tk.Label(
            self.pred_card,
            text=f"予測起床時刻: {pred_wake_time.strftime('%H:%M')}頃 ({method_ja})",
            font=("Yu Gothic Italic", 9),
            bg="#252538",
            fg="#a6adc8",
        ).pack(anchor="w", padx=15, pady=(0, 10))

        total_days = len(self.sessions)
        last_sleep = 0.0
        if total_days > 0:
            last_sleep = self.sessions[-1][2]

        # 集計期間でフィルタリング
        now_dt = datetime.now()
        period_cutoff = {
            "1w": now_dt - timedelta(days=7),
            "1m": now_dt - timedelta(days=30),
            "1y": now_dt - timedelta(days=365),
            "all": None,
        }
        cutoff = period_cutoff.get(self.stats_period)
        filtered = [
            s
            for s in self.sessions
            if cutoff is None or datetime.strptime(s[0], "%Y-%m-%d %H:%M:%S") >= cutoff
        ]
        avg_sleep = sum(s[2] for s in filtered) / len(filtered) if filtered else 0.0

        for widget in self.stats_card.winfo_children():
            widget.destroy()

        tk.Label(
            self.stats_card,
            text="睡眠の統計",
            font=("Yu Gothic UI", 11, "bold"),
            bg="#252538",
            fg="#bac2de",
        ).pack(anchor="w", padx=15, pady=(8, 2))
        tk.Label(
            self.stats_card,
            text=f"合計記録日数: {total_days} 日",
            font=("Yu Gothic", 10),
            bg="#252538",
            fg="#a6adc8",
        ).pack(anchor="w", padx=15)

        # 期間切り替えボタン
        period_frame = tk.Frame(self.stats_card, bg="#252538")
        period_frame.pack(anchor="w", padx=15, pady=(4, 0))
        period_labels = [
            ("先週", "1w"),
            ("先月", "1m"),
            ("一年", "1y"),
            ("全期間", "all"),
        ]
        for label, key in period_labels:
            is_active = self.stats_period == key
            btn = tk.Button(
                period_frame,
                text=label,
                font=("Yu Gothic UI", 8, "bold"),
                bg="#89b4fa" if is_active else "#313244",
                fg="#1e1e2e" if is_active else "#a6adc8",
                activebackground="#89b4fa",
                activeforeground="#1e1e2e",
                bd=0,
                padx=6,
                pady=2,
                cursor="hand2",
                command=lambda k=key: self._set_stats_period(k),
            )
            btn.pack(side="left", padx=(0, 3))

        period_ja = {
            "1w": "過去7日",
            "1m": "過去30日",
            "1y": "過去1年",
            "all": "全期間",
        }
        avg_str = f"平均睡眠時間 ({period_ja[self.stats_period]}): {int(avg_sleep)}時間 {int((avg_sleep % 1) * 60)}分"
        tk.Label(
            self.stats_card,
            text=avg_str,
            font=("Yu Gothic UI", 13, "bold"),
            bg="#252538",
            fg="#a6e3a1",
        ).pack(anchor="w", padx=15, pady=(4, 2))

        last_str = (
            f"前回の睡眠時間: {int(last_sleep)}時間 {int((last_sleep % 1) * 60)}分"
            if total_days > 0
            else "前回の睡眠時間: 記録なし"
        )
        tk.Label(
            self.stats_card,
            text=last_str,
            font=("Yu Gothic", 10),
            bg="#252538",
            fg="#cdd6f4",
        ).pack(anchor="w", padx=15, pady=(0, 8))

    def go_to_prev_week(self):
        self.current_week_start -= timedelta(days=7)
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def go_to_next_week(self):
        self.current_week_start += timedelta(days=7)
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def go_to_this_week(self):
        self.current_week_start = self.get_week_start_monday(datetime.now())
        self.date_var.set(self.current_week_start.strftime("%Y-%m-%d"))
        self.update_week_view()

    def update_week_view(self):
        self.sessions = database.get_all_sessions()
        week_end = self.current_week_start + timedelta(days=6)
        label_text = f"{self.current_week_start.strftime('%Y/%m/%d')} (月)  〜  {week_end.strftime('%Y/%m/%d')} (日)"
        self.week_label.config(text=label_text)

        if self.canvas:
            self.canvas.get_tk_widget().destroy()

        self.plot_weekly_graph()
        self.update_prediction_and_stats()

    def plot_weekly_graph(self):
        fig = Figure(figsize=(7, 4), dpi=100, facecolor="#252538")
        ax = fig.add_subplot(111)
        ax.set_facecolor("#252538")
        ax.grid(True, color="#313244", linestyle="--", linewidth=0.5, zorder=0)

        weekdays_ja = ["月", "火", "水", "木", "金", "土", "日"]
        days_in_week = [self.current_week_start + timedelta(days=i) for i in range(7)]
        xticklabels = [
            f"{w}\n({d.strftime('%m/%d')})" for w, d in zip(weekdays_ja, days_in_week)
        ]

        # 各日の最長セッションを「主睡眠」として就寝・起床時刻を取得
        durations = [0.0] * 7
        best_session = [None] * 7  # (duration, start_dt, end_dt)

        for start_str, end_str, dur, _ in self.sessions:
            try:
                start_dt = datetime.strptime(start_str, "%Y-%m-%d %H:%M:%S")
                end_dt = (
                    datetime.strptime(end_str, "%Y-%m-%d %H:%M:%S") if end_str else None
                )
                for idx, day in enumerate(days_in_week):
                    if start_dt.date() == day.date():
                        durations[idx] += dur
                        if best_session[idx] is None or dur > best_session[idx][0]:
                            best_session[idx] = (dur, start_dt, end_dt)
                        break
            except Exception:
                continue

        for sp in [ax.spines["top"], ax.spines["right"]]:
            sp.set_visible(False)
        ax.spines["bottom"].set_color("#45475a")
        ax.spines["left"].set_color("#45475a")
        ax.set_xticks(range(7))
        ax.set_xticklabels(
            xticklabels, color="#bac2de", fontsize=9, fontproperties="Yu Gothic"
        )
        ax.tick_params(colors="#bac2de", which="both", labelsize=10)
        ax.set_ylabel(
            "睡眠時間 (時間)", color="#bac2de", fontsize=10, fontproperties="Yu Gothic"
        )

        has_data = any(d > 0 for d in durations)
        if has_data:
            bars = ax.bar(
                range(7),
                durations,
                color="#89b4fa",
                width=0.55,
                edgecolor="#b4befe",
                linewidth=0.8,
                zorder=2,
            )
            for bar in bars:
                height = bar.get_height()
                if height > 0:
                    ax.annotate(
                        f"{height:.1f}h",
                        xy=(bar.get_x() + bar.get_width() / 2, height),
                        xytext=(0, 3),
                        textcoords="offset points",
                        ha="center",
                        va="bottom",
                        fontsize=8,
                        color="#cdd6f4",
                    )

            # 就寝・起床時刻の傾向ライン（右軸）
            ax2 = ax.twinx()
            ax2.set_facecolor("#252538")
            ax2.spines["top"].set_visible(False)
            ax2.spines["right"].set_color("#45475a")
            ax2.spines["left"].set_visible(False)
            ax2.spines["bottom"].set_visible(False)

            bed_x, bed_y, wake_x, wake_y = [], [], [], []
            for i, s in enumerate(best_session):
                if s is None:
                    continue
                _, start_dt, end_dt = s
                bh = start_dt.hour + start_dt.minute / 60
                if bh < 12:  # 深夜0-12時は24時以降として表示
                    bh += 24
                bed_x.append(i)
                bed_y.append(bh)
                if end_dt:
                    wh = end_dt.hour + end_dt.minute / 60
                    wake_x.append(i)
                    wake_y.append(wh)

            if bed_x:
                ax2.plot(
                    bed_x,
                    bed_y,
                    "o-",
                    color="#f9e2af",
                    linewidth=1.8,
                    markersize=5,
                    label="就寝",
                    zorder=3,
                )
            if wake_x:
                ax2.plot(
                    wake_x,
                    wake_y,
                    "s-",
                    color="#a6e3a1",
                    linewidth=1.8,
                    markersize=5,
                    label="起床",
                    zorder=3,
                )

            # 右軸のティック: 時刻表示（00:00=24, 01:00=25 …）
            ax2.set_ylim(3, 28)
            ax2.set_yticks([6, 8, 10, 20, 22, 24, 26])
            ax2.set_yticklabels(
                [f"{h%24:02d}:00" for h in [6, 8, 10, 20, 22, 24, 26]],
                color="#bac2de",
                fontsize=8,
            )
            ax2.set_ylabel(
                "就寝 / 起床時刻",
                color="#bac2de",
                fontsize=9,
                fontproperties="Yu Gothic",
            )
            ax2.tick_params(colors="#bac2de")

            if bed_x or wake_x:
                lines1, labels1 = ax2.get_legend_handles_labels()
                ax2.legend(
                    lines1,
                    labels1,
                    loc="upper right",
                    fontsize=8,
                    facecolor="#313244",
                    edgecolor="#45475a",
                    labelcolor="#cdd6f4",
                )
        else:
            ax.text(
                0.5,
                0.5,
                "この週の睡眠ログデータはありません。\n(グラフをクリックして手動で追加できます)",
                ha="center",
                va="center",
                color="#a6adc8",
                fontsize=10,
                transform=ax.transAxes,
                fontproperties="Yu Gothic",
            )
            ax.set_ylim(0, 10)

        fig.tight_layout()
        self.canvas = FigureCanvasTkAgg(fig, master=self.graph_frame)
        self.canvas.draw()
        self.canvas.get_tk_widget().pack(
            fill="both", expand=True, padx=10, pady=(0, 10)
        )
        self.canvas.mpl_connect("button_press_event", self.on_graph_click)

    def on_graph_click(self, event):
        if event.inaxes is None or event.xdata is None:
            return

        day_idx = int(round(event.xdata))
        if 0 <= day_idx < 7:
            target_date = self.current_week_start + timedelta(days=day_idx)
            SleepSessionDetailDialog(self.root, target_date, self.update_week_view)

    def show_connection_warning(self, reason: str):
        self.warning_label.config(
            text=f"⚠️ GitHub/Gistと同期できません ({reason})。ネット接続またはトークンを確認してください。"
        )
        self.warning_frame.pack(
            fill="x", padx=25, pady=(5, 5), before=self.summary_frame
        )

    def hide_connection_warning(self):
        self.warning_frame.pack_forget()

    def check_github_connection(self):
        def run_test():
            base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
            config_path = os.path.join(base_dir, "config.json")
            if not os.path.exists(config_path):
                self.root.after(
                    0,
                    lambda: self.show_connection_warning(
                        "設定ファイル config.json なし"
                    ),
                )
                return

            try:
                with open(config_path, "r", encoding="utf-8") as f:
                    config = json.load(f)
                    gist_id = config.get("gist_id")
                    token = config.get("github_token")
            except Exception:
                self.root.after(
                    0, lambda: self.show_connection_warning("設定ファイルの読込失敗")
                )
                return

            if not gist_id or not token:
                self.root.after(
                    0,
                    lambda: self.show_connection_warning("Gist IDまたはトークン未設定"),
                )
                return

            url = f"https://api.github.com/gists/{gist_id}"
            headers = {
                "Authorization": f"Bearer {token}",
                "Accept": "application/vnd.github+json",
                "User-Agent": "Sleep-Tracker-Client",
            }
            req = urllib.request.Request(url, headers=headers, method="GET")
            try:
                with urllib.request.urlopen(req, timeout=5) as response:
                    if response.status == 200:
                        self.root.after(0, self.hide_connection_warning)
                    else:
                        self.root.after(
                            0,
                            lambda: self.show_connection_warning(
                                f"HTTP {response.status}"
                            ),
                        )
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
    # ウィンドウ作成前に AUMID を設定しないとタスクバーが pythonw.exe のアイコングループに入る
    try:
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(
            "SleepTracker.UI.1"
        )
    except Exception:
        pass
    lifecycle.ensure_startup_registered()
    threading.Thread(target=lifecycle.register_start_menu_shortcut, daemon=True).start()
    root = tk.Tk()
    app = SleepTrackerApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
