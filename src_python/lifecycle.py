# File: src_python/lifecycle.py
# Description: Shared lifecycle synchronization helpers and paths for UI and monitor.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: is_monitor_running, ensure_monitor_running, check_process_exists, read_last_heartbeat
# Dependencies: os, sys, subprocess, datetime

import os
import sys
import subprocess
from datetime import datetime, timedelta

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG_DIR = os.path.join(BASE_DIR, "src_cpp")
PID_FILE = os.path.join(LOG_DIR, "monitor.pid")
HEARTBEAT_FILE = os.path.join(LOG_DIR, "sleep_heartbeat.txt")
EVENTS_FILE = os.path.join(LOG_DIR, "sleep_events.txt")
MONITOR_PATH = os.path.join(BASE_DIR, "src_python", "monitor.py")

def read_last_heartbeat() -> tuple[datetime, int] | None:
    """最後の生存ハートビートを読み込む (時刻, アイドル時間[ms])"""
    if not os.path.exists(HEARTBEAT_FILE):
        return None
    try:
        with open(HEARTBEAT_FILE, "r", encoding="utf-8") as f:
            line = f.readline().strip()
            if not line:
                return None
            parts = line.split(",")
            if len(parts) == 2:
                ts = datetime.strptime(parts[0].strip(), "%Y-%m-%d %H:%M:%S")
                idle_ms = int(parts[1])
                return ts, idle_ms
    except Exception:
        pass
    return None

def is_monitor_running() -> tuple[bool, str]:
    """監視サービスが稼働しているかをハートビートファイルから確認する"""
    hb_info = read_last_heartbeat()
    if not hb_info:
        return False, "停止中 (生存信号なし)"
    
    hb_time, _ = hb_info
    if datetime.now() - hb_time < timedelta(minutes=3):
        return True, f"稼働中 (最終更新: {hb_time.strftime('%H:%M:%S')})"
    else:
        return False, f"停止中 (最終更新: {hb_time.strftime('%m-%d %H:%M')})"

def ensure_monitor_running():
    """バックグラウンド監視モニターが稼働していない場合、自動起動する"""
    is_running, _ = is_monitor_running()
    if not is_running:
        pythonw_exe = os.path.join(BASE_DIR, ".venv", "Scripts", "pythonw.exe")
        if not os.path.exists(pythonw_exe):
            pythonw_exe = sys.executable.replace("python.exe", "pythonw.exe")
        
        try:
            # 親の死による巻き添え終了を防ぐため PowerShell の Start-Process 経由で独立起動
            ps_cmd = f"Start-Process -WindowStyle Hidden -FilePath '{pythonw_exe}' -ArgumentList '\"{MONITOR_PATH}\"'"
            subprocess.Popen(["powershell", "-Command", ps_cmd], creationflags=subprocess.CREATE_NO_WINDOW)
            print("Auto-started background monitor.py successfully.")
        except Exception as e:
            print(f"Failed to auto-start monitor.py: {e}")

def check_process_exists(pid: int) -> bool:
    """指定された PID のプロセスが OS 上にアクティブに存在するか確認する"""
    try:
        res = subprocess.run(
            ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
            capture_output=True, text=True,
            creationflags=subprocess.CREATE_NO_WINDOW
        )
        return "No tasks are running" not in res.stdout and str(pid) in res.stdout
    except Exception:
        return False
