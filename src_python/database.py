# File: src_python/database.py
# Description: Processes raw logs, extracts sleep sessions, and manages SQLite database with auto-git push.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: init_db, get_db_connection, sync_logs_to_db, get_all_sessions, git_push_logs
# Dependencies: sqlite3, os, datetime, subprocess, threading

import sqlite3
import os
import subprocess
import threading
from datetime import datetime, timedelta

# プロジェクトルートディレクトリの設定
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG_DIR = os.path.join(BASE_DIR, "src_cpp")
DB_PATH = os.path.join(BASE_DIR, "sleep_tracker.db")

EVENTS_FILE = os.path.join(LOG_DIR, "sleep_events.txt")
HEARTBEAT_FILE = os.path.join(LOG_DIR, "sleep_heartbeat.txt")

def get_db_connection():
    """SQLite データベースへの接続を取得する"""
    return sqlite3.connect(DB_PATH)

def init_db():
    """データベースとテーブルの初期化"""
    conn = get_db_connection()
    cursor = conn.cursor()
    cursor.execute("""
        CREATE TABLE IF NOT EXISTS sleep_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            start_time TEXT UNIQUE,
            end_time TEXT,
            duration_hours REAL,
            session_type TEXT
        )
    """)
    conn.commit()
    conn.close()

def parse_datetime(dt_str: str) -> datetime:
    """文字列から datetime オブジェクトをパースする"""
    return datetime.strptime(dt_str.strip(), "%Y-%m-%d %H:%M:%S")

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
                ts = parse_datetime(parts[0])
                idle_ms = int(parts[1])
                return ts, idle_ms
    except Exception:
        pass
    return None

def sync_logs_to_db():
    """生ログを解析してデータベースに同期する"""
    if not os.path.exists(EVENTS_FILE):
        return

    # 1. ログファイルの読み込みとパース
    events = []
    try:
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                parts = line.split(",", 1)
                if len(parts) == 2:
                    try:
                        ts = parse_datetime(parts[0])
                        event_type = parts[1]
                        events.append((ts, event_type))
                    except ValueError:
                        continue
    except Exception as e:
        print(f"Error reading events file: {e}")
        return

    if not events:
        return

    # 時系列順にソート
    events.sort(key=lambda x: x[0])

    conn = get_db_connection()
    cursor = conn.cursor()

    # 2. 状態遷移マシンによる睡眠セッションの抽出
    # 状態: 'ACTIVE' (PC使用中), 'SLEEPING' (PC未使用/オフ)
    state = 'ACTIVE'
    sleep_start = None
    session_type = None

    # 最小の睡眠判定時間 (例: 30分)
    MIN_SLEEP_DURATION = timedelta(minutes=30)

    for i, (ts, event) in enumerate(events):
        if state == 'ACTIVE':
            if event in ('IDLE_START', 'SUSPEND', 'SHUTDOWN'):
                state = 'SLEEPING'
                sleep_start = ts
                session_type = 'IDLE' if event == 'IDLE_START' else 'POWER'
            
            # 突然の電源断の検出 (直前イベントがACTIVEなのに突如STARTUP/RESUMEが来た場合)
            elif event in ('STARTUP', 'RESUME') and i > 0:
                # 直前のイベントからギャップがあるか確認する
                # 前回のイベントから 4時間以上 経過している場合は強制シャットダウンと仮定
                prev_ts, prev_event = events[i-1]
                if ts - prev_ts > timedelta(hours=4):
                    # 最後のハートビート情報があればそれを開始時刻に補正
                    hb_info = read_last_heartbeat()
                    if hb_info and prev_ts < hb_info[0] < ts:
                        # 最後の操作時刻 = ハートビート時刻 - アイドル時間
                        last_active = hb_info[0] - timedelta(milliseconds=hb_info[1])
                        # ただし、前回のイベント時刻よりは未来であること
                        start_time = max(prev_ts, last_active)
                    else:
                        start_time = prev_ts

                    duration = ts - start_time
                    if duration >= MIN_SLEEP_DURATION:
                        duration_hours = duration.total_seconds() / 3600.0
                        try:
                            cursor.execute("""
                                INSERT OR IGNORE INTO sleep_sessions 
                                (start_time, end_time, duration_hours, session_type)
                                VALUES (?, ?, ?, ?)
                            """, (
                                start_time.strftime("%Y-%m-%d %H:%M:%S"),
                                ts.strftime("%Y-%m-%d %H:%M:%S"),
                                duration_hours,
                                'POWER_LOSS'
                            ))
                        except sqlite3.Error:
                            pass

        elif state == 'SLEEPING':
            # 睡眠終了イベント
            if event in ('IDLE_RESUME', 'RESUME', 'STARTUP'):
                duration = ts - sleep_start
                if duration >= MIN_SLEEP_DURATION:
                    duration_hours = duration.total_seconds() / 3600.0
                    try:
                        cursor.execute("""
                            INSERT OR IGNORE INTO sleep_sessions 
                            (start_time, end_time, duration_hours, session_type)
                            VALUES (?, ?, ?, ?)
                        """, (
                            sleep_start.strftime("%Y-%m-%d %H:%M:%S"),
                            ts.strftime("%Y-%m-%d %H:%M:%S"),
                            duration_hours,
                            session_type
                        ))
                    except sqlite3.Error:
                        pass
                state = 'ACTIVE'
                sleep_start = None
                session_type = None
            
            # すでにSLEEPING状態で、さらに深い電源状態（SUSPEND/SHUTDOWN）に入った場合は
            # 開始時刻は維持しつつタイプを更新
            elif event in ('SUSPEND', 'SHUTDOWN'):
                session_type = 'POWER'

    conn.commit()
    conn.close()

    # ログ変更の自動 Git Push を非同期で実行
    git_push_logs()

def git_push_logs():
    """非同期でログファイルを Git リポジトリに push する"""
    def _push():
        try:
            # 変更があるか確認 (--porcelain)
            res = subprocess.run(
                ["git", "status", "--porcelain", EVENTS_FILE],
                capture_output=True, text=True, cwd=BASE_DIR
            )
            if not res.stdout.strip():
                return
            
            # コミット対象に追加
            subprocess.run(["git", "add", EVENTS_FILE], cwd=BASE_DIR, check=True)
            timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            subprocess.run(
                ["git", "commit", "-m", f"Auto-update sleep logs: {timestamp}"],
                cwd=BASE_DIR, check=True
            )
            
            # リモート競合対策でリベースプルを行い、その後にプッシュ
            subprocess.run(["git", "pull", "--rebase", "origin", "master"], cwd=BASE_DIR, check=True)
            subprocess.run(["git", "push", "origin", "master"], cwd=BASE_DIR, check=True)
            print(f"[{timestamp}] Sleep logs successfully pushed to GitHub.")
        except Exception as e:
            # 常駐プロセスなのでエラーで落ちないようにキャッチするだけにする
            print(f"Failed to auto-push logs to Git: {e}")

    thread = threading.Thread(target=_push, daemon=True)
    thread.start()

def get_all_sessions():
    """保存されているすべての睡眠セッションを取得する"""
    conn = get_db_connection()
    cursor = conn.cursor()
    cursor.execute("""
        SELECT start_time, end_time, duration_hours, session_type 
        FROM sleep_sessions 
        ORDER BY start_time ASC
    """)
    rows = cursor.fetchall()
    conn.close()
    return rows
