# File: src_python/monitor.py
# Description: Python fallback monitor to track system idle time and heartbeats, with automatic hourly Gist sync.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: get_idle_duration_ms, log_event, update_heartbeat, monitor_loop, main
# Dependencies: ctypes, time, os, sys, datetime, threading, database

import sys
import os

# 実行時作業ディレクトリをスクリプトの場所基準でリポジトリルートに強制固定 (Windows特有のSystem32起動時クラッシュを100%防止)
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_DIR = os.path.dirname(SCRIPT_DIR)
os.chdir(BASE_DIR)

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
    log_event("STARTUP")
    
    # データベースの初期化と、起動時の初回Gist自動同期（非同期）
    database.init_db()
    try:
        threading.Thread(target=database.sync_logs_to_db, daemon=True).start()
    except Exception:
        pass

    # 監視メインループを実行
    monitor_loop()

if __name__ == "__main__":
    main()
