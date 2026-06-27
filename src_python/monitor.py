# File: src_python/monitor.py
# Description: Background monitor tracking system idle time and heartbeats, with tray icon and hourly Gist sync.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: get_idle_duration_ms, log_event, update_heartbeat, monitor_loop, main

import sys
import os
import subprocess

# 実行時作業ディレクトリをスクリプトの場所基準でリポジトリルートに強制固定 (Windows特有のSystem32起動時クラッシュを100%防止)
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(SCRIPT_DIR)
os.chdir(BASE_DIR)

# .venv 以外の Python で起動された場合は .venv の pythonw.exe で自分自身を再起動する
# (pystray 等の依存パッケージは .venv にしかインストールされていないため)
_VENV_PYTHONW = os.path.join(BASE_DIR, ".venv", "Scripts", "pythonw.exe")
if os.path.exists(_VENV_PYTHONW) and os.path.abspath(sys.executable).lower() != os.path.abspath(_VENV_PYTHONW).lower():
    subprocess.Popen(
        [_VENV_PYTHONW, os.path.abspath(__file__)],
        cwd=BASE_DIR,
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    sys.exit(0)

# pythonw.exe (コンソール非表示) 起動時における sys.stdout/stderr への print/write によるクラッシュを完全に防止
class DummyStream:
    def write(self, *args, **kwargs): pass
    def flush(self, *args, **kwargs): pass
    encoding = "utf-8"
    errors = "ignore"

if sys.stdout is None:
    sys.stdout = DummyStream()
if sys.stderr is None:
    sys.stderr = DummyStream()

import ctypes
import time
from datetime import datetime
import threading

import database

# Windows API 用の構造体定義
class LASTINPUTINFO(ctypes.Structure):
    _fields_ = [
        ("cbSize", ctypes.c_uint),
        ("dwTime", ctypes.c_uint)
    ]

LOG_DIR = os.path.join(BASE_DIR, "src_cpp")
os.makedirs(LOG_DIR, exist_ok=True)

EVENTS_FILE = os.path.join(LOG_DIR, "sleep_events.txt")
HEARTBEAT_FILE = os.path.join(LOG_DIR, "sleep_heartbeat.txt")
PID_FILE = os.path.join(LOG_DIR, "monitor.pid")

# タスクトレイアイコン (グローバル参照)
_tray_icon = None

def get_idle_duration_ms() -> int:
    """Windows API を使用して、最後の入力から経過した時間（ミリ秒）を取得する"""
    lii = LASTINPUTINFO()
    lii.cbSize = ctypes.sizeof(LASTINPUTINFO)
    if ctypes.windll.user32.GetLastInputInfo(ctypes.byref(lii)):
        tick_count = ctypes.windll.kernel32.GetTickCount()
        elapsed = tick_count - lii.dwTime
        if elapsed >= 0:
            return elapsed
        else:
            return (0xFFFFFFFF - lii.dwTime) + tick_count
    return 0

def log_event(event_type: str, timestamp: str = None):
    """イベントログ（起動、終了など）を sleep_events.txt に記録する"""
    if timestamp is None:
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    try:
        with open(EVENTS_FILE, "a", encoding="utf-8") as f:
            f.write(f"{timestamp},{event_type}\n")
    except Exception as e:
        print(f"Error logging event: {e}")

def update_heartbeat(idle_ms: int):
    """生存信号（ハートビート）を sleep_heartbeat.txt に上書き保存する"""
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    try:
        with open(HEARTBEAT_FILE, "w", encoding="utf-8") as f:
            f.write(f"{timestamp},{idle_ms}\n")
    except Exception as e:
        print(f"Error updating heartbeat: {e}")

def open_main_ui(icon=None, item=None):
    """メイン UI (main.py) を別プロセスで起動する"""
    main_path = os.path.join(SCRIPT_DIR, "main.py")
    python_exe = sys.executable
    # pythonw.exe で起動していた場合も GUI は python.exe で開く
    if python_exe.lower().endswith("pythonw.exe"):
        python_exe = python_exe[:-len("pythonw.exe")] + "python.exe"
    try:
        subprocess.Popen(
            [python_exe, main_path],
            cwd=BASE_DIR,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
    except Exception as e:
        print(f"Failed to open main UI: {e}")

def quit_app(icon=None, item=None):
    """タスクトレイアイコンを停止してプロセスを終了する"""
    global _tray_icon
    log_event("SHUTDOWN")
    try:
        os.remove(PID_FILE)
    except Exception:
        pass
    if _tray_icon:
        _tray_icon.stop()

def build_tray_icon():
    """pystray のタスクトレイアイコンを構築して返す"""
    try:
        import pystray
        from PIL import Image

        ico_path = os.path.join(SCRIPT_DIR, "sleep_tracker.ico")
        if os.path.exists(ico_path):
            image = Image.open(ico_path)
        else:
            # .ico がない場合は三日月形を描画してフォールバック
            image = Image.new("RGBA", (64, 64), (0, 0, 0, 0))
            from PIL import ImageDraw
            draw = ImageDraw.Draw(image)
            draw.ellipse([4, 4, 60, 60], fill=(30, 30, 46, 255))
            draw.ellipse([16, 4, 72, 60], fill=(0, 0, 0, 0))

        menu = pystray.Menu(
            pystray.MenuItem("睡眠トラッカーを開く", open_main_ui, default=True),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("終了", quit_app),
        )
        icon = pystray.Icon("sleep_tracker", image, "睡眠トラッカー 監視中", menu)
        return icon
    except Exception as e:
        log_event(f"TRAY_ERROR: {str(e)[:80]}")
        return None

def monitor_loop():
    """バックグラウンドでシステムアイドル時間を監視し続けるメインループ"""
    is_idle = False
    idle_threshold_ms = 20 * 60 * 1000  # 20分
    loop_count = 0

    try:
        while True:
            idle_ms = get_idle_duration_ms()
            update_heartbeat(idle_ms)

            if idle_ms >= idle_threshold_ms:
                if not is_idle:
                    is_idle = True
                    start_time = datetime.fromtimestamp(time.time() - (idle_ms / 1000.0))
                    time_str = start_time.strftime("%Y-%m-%d %H:%M:%S")
                    log_event("IDLE_START", time_str)
            else:
                if is_idle:
                    is_idle = False
                    log_event("IDLE_RESUME")

            time.sleep(60)  # 1分待機
            loop_count += 1

            # 1時間（60ループ）ごとに自動でGist同期（非同期）を実行
            if loop_count >= 60:
                loop_count = 0
                try:
                    threading.Thread(target=database.sync_logs_to_db, daemon=True).start()
                except Exception:
                    pass

    except Exception as e:
        log_event(f"ERROR: {str(e)[:50]}")

def main():
    global _tray_icon

    # 自分の PID をファイルに書いて、生存確認できるようにする
    try:
        with open(PID_FILE, "w") as f:
            f.write(str(os.getpid()))
    except Exception:
        pass

    log_event("STARTUP")

    # データベースの初期化と、起動時の初回Gist自動同期（非同期）
    database.init_db()
    try:
        threading.Thread(target=database.sync_logs_to_db, daemon=True).start()
    except Exception:
        pass

    # 監視ループを別スレッドで起動
    monitor_thread = threading.Thread(target=monitor_loop, daemon=True)
    monitor_thread.start()

    # タスクトレイアイコンをメインスレッドで実行 (pystray はメインスレッド必須)
    _tray_icon = build_tray_icon()
    if _tray_icon:
        _tray_icon.run()
    else:
        # pystray が使えない環境ではスレッドが終わるまで待機
        monitor_thread.join()

if __name__ == "__main__":
    main()
