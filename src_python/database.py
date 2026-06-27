# File: src_python/database.py
# Description: Processes raw logs, SQLite DB, and synchronizes GitHub Gist mobile events & auto-git push.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: init_db, get_db_connection, sync_logs_to_db, get_all_sessions, git_push_logs, sync_mobile_events_from_gist
# Dependencies: sqlite3, os, datetime, subprocess, threading, json, urllib.request

import sqlite3
import os
import subprocess
import threading
import json
import urllib.request
from datetime import datetime, timedelta

# プロジェクトルートディレクトリの設定
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG_DIR = os.path.join(BASE_DIR, "src_cpp")
DB_PATH = os.path.join(BASE_DIR, "sleep_tracker.db")

EVENTS_FILE = os.path.join(LOG_DIR, "sleep_events.txt")
HEARTBEAT_FILE = os.path.join(LOG_DIR, "sleep_heartbeat.txt")
CONFIG_PATH = os.path.join(BASE_DIR, "config.json")

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

def sync_mobile_events_from_gist():
    """GitHub Gist から iPhone が記録した外出/帰宅イベントを取得してマージする"""
    if not os.path.exists(CONFIG_PATH):
        return
        
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as f:
            config = json.load(f)
            gist_id = config.get("gist_id")
            github_token = config.get("github_token")
    except Exception:
        return
        
    if not gist_id:
        return
        
    # GitHub トークンの取得 (config.jsonのトークンを優先し、無ければ gh CLI から取得)
    token = github_token
    if not token:
        try:
            res = subprocess.run(
                ["gh", "auth", "token"],
                capture_output=True, text=True, check=True,
                creationflags=subprocess.CREATE_NO_WINDOW
            )
            token = res.stdout.strip()
        except Exception:
            pass
            
    if not token:
        return
        
    # Gist API からのフェッチ
    url = f"https://api.github.com/gists/{gist_id}"
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "Sleep-Tracker-Client"
    }
    
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            files = res_data.get("files", {})
            mobile_file = files.get("mobile_event.txt", {})
            content = mobile_file.get("content", "").strip()
            
            if not content or content.startswith("INIT"):
                return
                
            # パース例: LEAVE,2026-06-27 15:30:00
            parts = content.split(",")
            if len(parts) == 2:
                event_type = "OUT_START" if parts[0] == "LEAVE" else "OUT_END"
                event_time_str = parts[1]
                
                # 新しいイベントか確認
                new_event_line = f"{event_time_str},{event_type}\n"
                
                # sleep_events.txt にすでに同一のイベントが含まれているか確認
                already_exists = False
                if os.path.exists(EVENTS_FILE):
                    with open(EVENTS_FILE, "r", encoding="utf-8") as ef:
                        for line in ef:
                            if line.strip() == f"{event_time_str},{event_type}":
                                already_exists = True
                                break
                                
                # 存在しない場合のみ追記
                if not already_exists:
                    with open(EVENTS_FILE, "a", encoding="utf-8") as ef:
                        ef.write(new_event_line)
                    print(f"Synced mobile event from Gist: {event_type} at {event_time_str}")
    except Exception as e:
        print(f"Failed to fetch mobile events from Gist: {e}")

def clear_all_data():
    """生ログファイル、SQLiteデータベース、およびGist上のログを完全に初期化する"""
    # 1. sleep_events.txt を空にする
    try:
        with open(EVENTS_FILE, "w", encoding="utf-8") as f:
            f.write("")
    except Exception as e:
        print(f"Failed to clear events file: {e}")
        
    # 2. SQLite データベースの sleep_sessions テーブルをクリア
    try:
        conn = get_db_connection()
        cursor = conn.cursor()
        cursor.execute("DELETE FROM sleep_sessions")
        conn.commit()
        conn.close()
    except Exception as e:
        print(f"Failed to clear SQLite DB: {e}")
        
    # 3. Gist の mobile_event.txt を "INIT" に上書きして初期化
    try:
        if os.path.exists(CONFIG_PATH):
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                config = json.load(f)
                gist_id = config.get("gist_id")
                token = config.get("github_token")
                
            if gist_id and token:
                url = f"https://api.github.com/gists/{gist_id}"
                headers = {
                    "Authorization": f"Bearer {token}",
                    "Accept": "application/vnd.github+json",
                    "X-GitHub-Api-Version": "2022-11-28",
                    "User-Agent": "Sleep-Tracker-Client"
                }
                # Gist初期化用JSONデータ
                data = {
                    "files": {
                        "mobile_event.txt": {
                            "content": "INIT"
                        }
                    }
                }
                req = urllib.request.Request(
                    url, 
                    data=json.dumps(data).encode("utf-8"), 
                    headers=headers, 
                    method="PATCH"
                )
                with urllib.request.urlopen(req, timeout=5) as response:
                    pass
    except Exception as e:
        print(f"Failed to reset Gist: {e}")
        
    # 4. Git への同期プッシュ (空になったことをリポジトリに反映)
    try:
        git_push_logs()
    except Exception as e:
        print(f"Failed to push cleared logs to git: {e}")

def sync_logs_to_db():
    """生ログを解析してデータベースに同期する"""
    # 0a. sleep_events.txt が存在しない場合（新PC・移植時）は Gist からダウンロード
    if not os.path.exists(EVENTS_FILE):
        _download_events_from_gist()

    # 0b. Gist からモバイルの外出/帰宅イベントを取得してマージ
    sync_mobile_events_from_gist()

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
    is_out = False  # 外出中フラグ

    # 最小睡眠セッション時間 = idle閾値と同じ（設定変更を即反映）
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as _f:
            _threshold_minutes = max(1, int(json.load(_f).get("idle_threshold_minutes", 30)))
    except Exception:
        _threshold_minutes = 30
    MIN_SLEEP_DURATION = timedelta(minutes=_threshold_minutes)

    for i, (ts, event) in enumerate(events):
        # 外出状態の更新
        if event == 'OUT_START':
            is_out = True
            # もし現在睡眠中なら、外出が始まった時点で強制的に睡眠をクローズする
            if state == 'SLEEPING':
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
            continue
        elif event == 'OUT_END':
            is_out = False
            continue

        if state == 'ACTIVE':
            # 外出中でない場合のみ睡眠を開始する
            if not is_out and event in ('IDLE_START', 'SUSPEND', 'SHUTDOWN'):
                state = 'SLEEPING'
                sleep_start = ts
                session_type = 'IDLE' if event == 'IDLE_START' else 'POWER'
            
            # 突然の電源断の検出 (直前イベントがACTIVEなのに突如STARTUP/RESUMEが来た場合)
            elif event in ('STARTUP', 'RESUME') and i > 0:
                # 外出中でない場合のみ電源断睡眠を記録
                if not is_out:
                    prev_ts, prev_event = events[i-1]
                    if ts - prev_ts > timedelta(hours=4):
                        # 最後のハートビート情報があればそれを開始時刻に補正
                        hb_info = read_last_heartbeat()
                        if hb_info and prev_ts < hb_info[0] < ts:
                            last_active = hb_info[0] - timedelta(milliseconds=hb_info[1])
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
            
            # すでにSLEEPING状態で、さらに深い電源状態（SUSPEND/SHUTDOWN）に入った場合はタイプ更新
            elif event in ('SUSPEND', 'SHUTDOWN'):
                session_type = 'POWER'

    conn.commit()
    conn.close()

    # ログ変更の自動 Git Push を非同期で実行
    git_push_logs()

def _get_gist_config() -> tuple[str, str] | tuple[None, None]:
    """config.json から gist_id と github_token を読み込む"""
    if not os.path.exists(CONFIG_PATH):
        return None, None
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as f:
            config = json.load(f)
        return config.get("gist_id"), config.get("github_token")
    except Exception:
        return None, None

def _push_events_to_gist():
    """sleep_events.txt の内容を Gist に PATCH する（同期・内部用）"""
    if not os.path.exists(EVENTS_FILE):
        return
    gist_id, token = _get_gist_config()
    if not gist_id or not token:
        return
    try:
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            content = f.read()
        url = f"https://api.github.com/gists/{gist_id}"
        headers = {
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "Sleep-Tracker-Client"
        }
        data = {"files": {"sleep_events.txt": {"content": content if content.strip() else " "}}}
        req = urllib.request.Request(
            url, data=json.dumps(data).encode("utf-8"), headers=headers, method="PATCH"
        )
        with urllib.request.urlopen(req, timeout=10) as _:
            pass
        print(f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] sleep_events.txt pushed to Gist.")
    except Exception as e:
        print(f"Failed to push events to Gist: {e}")

def validate_gist_connection():
    """Gist IDとトークンの有効性を確認する。失敗時は ValueError を送出する"""
    import urllib.error
    gist_id, token = _get_gist_config()
    if not gist_id:
        raise ValueError("Gist IDが設定されていません。")
    if not token:
        raise ValueError("GitHub トークンが設定されていません。")
    url = f"https://api.github.com/gists/{gist_id}"
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "Sleep-Tracker-Client",
    }
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=10):
            pass
    except urllib.error.HTTPError as e:
        if e.code == 404:
            raise ValueError(f"Gist IDが見つかりません (404):\n{gist_id}") from e
        elif e.code == 401:
            raise ValueError("GitHub トークンが無効です (401)。") from e
        else:
            raise ValueError(f"Gist API エラー (HTTP {e.code})。") from e
    except urllib.error.URLError as e:
        raise ValueError(f"ネットワークエラー: {e.reason}") from e

def _download_events_from_gist():
    """Gist から sleep_events.txt をダウンロードしてローカルに書き出す（移植・初回起動時用）"""
    gist_id, token = _get_gist_config()
    if not gist_id or not token:
        return
    try:
        url = f"https://api.github.com/gists/{gist_id}"
        headers = {
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "Sleep-Tracker-Client"
        }
        req = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(req, timeout=10) as response:
            data = json.loads(response.read().decode("utf-8"))
            content = data.get("files", {}).get("sleep_events.txt", {}).get("content", "").strip()
            if content and content != " ":
                os.makedirs(LOG_DIR, exist_ok=True)
                with open(EVENTS_FILE, "w", encoding="utf-8") as f:
                    f.write(content)
                print("sleep_events.txt restored from Gist.")
    except Exception as e:
        print(f"Failed to download events from Gist: {e}")

def get_current_out_state() -> bool:
    """sleep_events.txt の最後の OUT_START/OUT_END から現在の検知中断状態を返す"""
    if not os.path.exists(EVENTS_FILE):
        return False
    last_out = None
    try:
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            for line in f:
                s = line.strip()
                if ",OUT_START" in s or ",OUT_END" in s:
                    last_out = s
    except Exception:
        pass
    return last_out is not None and ",OUT_START" in last_out

def record_out_event(event_type: str):
    """OUT_START / OUT_END を sleep_events.txt に直接記録し Gist へ非同期バックアップする"""
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    try:
        os.makedirs(LOG_DIR, exist_ok=True)
        with open(EVENTS_FILE, "a", encoding="utf-8") as f:
            f.write(f"{timestamp},{event_type}\n")
    except Exception as e:
        print(f"Failed to record out event: {e}")
        return
    git_push_logs()

def git_push_logs(wait: bool = False):
    """sleep_events.txt を Gist に非同期バックアップする。wait=True で最大8秒ブロック（終了時用）"""
    t = threading.Thread(target=_push_events_to_gist, daemon=True)
    t.start()
    if wait:
        t.join(timeout=8)

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

def get_sessions_with_ids():
    """IDを含むすべての睡眠セッションを取得する"""
    conn = get_db_connection()
    cursor = conn.cursor()
    cursor.execute("""
        SELECT id, start_time, end_time, duration_hours, session_type 
        FROM sleep_sessions 
        ORDER BY start_time ASC
    """)
    rows = cursor.fetchall()
    conn.close()
    return rows

def rebuild_events_file_from_db():
    """SQLite データベースの全セッションと、既存のシステムログから sleep_events.txt を再構築する"""
    system_events = []
    
    if os.path.exists(EVENTS_FILE):
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
                            if event_type in ("STARTUP", "SHUTDOWN") or event_type.startswith("ERROR") or event_type.startswith("TRAY_ERROR"):
                                system_events.append((ts, f"{parts[0]},{event_type}\n"))
                        except ValueError:
                            continue
        except Exception as e:
            print(f"Error reading events file for rebuild: {e}")

    session_events = []
    try:
        conn = get_db_connection()
        cursor = conn.cursor()
        cursor.execute("SELECT start_time, end_time, session_type FROM sleep_sessions")
        rows = cursor.fetchall()
        conn.close()
        
        for start_time_str, end_time_str, s_type in rows:
            try:
                start_dt = parse_datetime(start_time_str)
                start_ev = "IDLE_START" if s_type == "sleep" else "OUT_START"
                session_events.append((start_dt, f"{start_time_str},{start_ev}\n"))
                
                if end_time_str:
                    end_dt = parse_datetime(end_time_str)
                    end_ev = "IDLE_RESUME" if s_type == "sleep" else "OUT_END"
                    session_events.append((end_dt, f"{end_time_str},{end_ev}\n"))
            except ValueError:
                continue
    except Exception as e:
        print(f"Error reading DB for rebuild: {e}")

    all_events = system_events + session_events
    all_events.sort(key=lambda x: x[0])

    try:
        with open(EVENTS_FILE, "w", encoding="utf-8") as f:
            for _, line_text in all_events:
                f.write(line_text)
        print("Successfully rebuilt sleep_events.txt from DB sessions.")
    except Exception as e:
        print(f"Error writing rebuilt events file: {e}")

def delete_session_and_rebuild(session_id: int) -> bool:
    """指定されたIDの睡眠セッションを削除し、sleep_events.txt を再構築してプッシュする"""
    try:
        conn = get_db_connection()
        cursor = conn.cursor()
        cursor.execute("DELETE FROM sleep_sessions WHERE id = ?", (session_id,))
        conn.commit()
        conn.close()
        
        rebuild_events_file_from_db()
        git_push_logs()
        return True
    except Exception as e:
        print(f"Error deleting session: {e}")
        return False

def bulk_import_sessions(sessions: list) -> tuple[int, int]:
    """CSVなどから複数セッションを一括挿入する。重複は INSERT OR IGNORE でスキップ。
    Returns (imported_count, skipped_count)"""
    imported = 0
    skipped = 0
    conn = get_db_connection()
    cursor = conn.cursor()
    for start_str, end_str, stype in sessions:
        try:
            start_dt = parse_datetime(start_str)
            end_dt = parse_datetime(end_str)
            if start_dt >= end_dt:
                skipped += 1
                continue
            duration_hours = (end_dt - start_dt).total_seconds() / 3600.0
            cursor.execute("""
                INSERT OR IGNORE INTO sleep_sessions (start_time, end_time, duration_hours, session_type)
                VALUES (?, ?, ?, ?)
            """, (start_str, end_str, duration_hours, stype or "sleep"))
            if cursor.rowcount > 0:
                imported += 1
            else:
                skipped += 1
        except Exception:
            skipped += 1
    conn.commit()
    conn.close()
    if imported > 0:
        rebuild_events_file_from_db()
        git_push_logs()
    return imported, skipped

def add_session_and_rebuild(start_time_str: str, end_time_str: str, session_type: str = "sleep") -> tuple[bool, str]:
    """新しい睡眠セッションを手動で挿入し、sleep_events.txt を再構築してプッシュする"""
    try:
        start_dt = parse_datetime(start_time_str)
        end_dt = parse_datetime(end_time_str)
        
        if start_dt >= end_dt:
            return False, "開始時刻は終了時刻より前である必要があります。"

        conn = get_db_connection()
        cursor = conn.cursor()
        
        cursor.execute("SELECT start_time, end_time FROM sleep_sessions")
        rows = cursor.fetchall()
        for s_str, e_str in rows:
            if not e_str:
                continue
            s_dt = parse_datetime(s_str)
            e_dt = parse_datetime(e_str)
            if (start_dt < e_dt) and (s_dt < end_dt):
                conn.close()
                return False, "既存の睡眠記録と時間帯が重複しています。"

        duration_hours = (end_dt - start_dt).total_seconds() / 3600.0
        
        cursor.execute("""
            INSERT INTO sleep_sessions (start_time, end_time, duration_hours, session_type)
            VALUES (?, ?, ?, ?)
        """, (start_time_str, end_time_str, duration_hours, session_type))
        conn.commit()
        conn.close()
        
        rebuild_events_file_from_db()
        git_push_logs()
        return True, "成功"
    except Exception as e:
        print(f"Error adding session: {e}")
        return False, str(e)
