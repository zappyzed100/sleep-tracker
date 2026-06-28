# File: src_python/database.py
# Description: Parses sleep_events.txt directly for sessions, synchronizes GitHub Gist mobile events & auto-git push.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: get_all_sessions, get_sessions_with_ids, add_session_and_rebuild, delete_session_and_rebuild, sync_with_gist, git_push_logs, sync_mobile_events_from_gist

import os
import subprocess
import threading
import json
import urllib.request
from datetime import datetime, timedelta

# プロジェクトルートディレクトリの設定
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG_DIR = os.path.join(BASE_DIR, "src_cpp")

EVENTS_FILE = os.path.join(LOG_DIR, "sleep_events.txt")
HEARTBEAT_FILE = os.path.join(LOG_DIR, "sleep_heartbeat.txt")
DEVICE_HEARTBEAT_FILE = os.path.join(LOG_DIR, "device_heartbeat.txt")
CONFIG_PATH = os.path.join(BASE_DIR, "config.json")

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

def _load_min_duration() -> timedelta:
    """config.json から idle_threshold_minutes を読み込んで timedelta で返す"""
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as f:
            minutes = max(1, int(json.load(f).get("idle_threshold_minutes", 30)))
    except Exception:
        minutes = 30
    return timedelta(minutes=minutes)

_PARSE_EXE = os.path.join(LOG_DIR, "parse_sessions.exe")

def _parse_events_to_sessions() -> list:
    """sleep_events.txt を状態遷移マシンで解析して睡眠セッションのリストを返す。
    parse_sessions.exe が存在すれば C++ 実装を呼び出し、なければ Python フォールバックを使う。
    各要素: (start_time_str, end_time_str, duration_hours, session_type)
    """
    if os.path.exists(_PARSE_EXE):
        try:
            result = subprocess.run(
                [_PARSE_EXE, EVENTS_FILE, HEARTBEAT_FILE, CONFIG_PATH],
                capture_output=True, text=True, timeout=10,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )
            if result.returncode == 0:
                rows = json.loads(result.stdout)
                return [(r["start"], r["end"], r["duration"], r["type"]) for r in rows]
            print(f"parse_sessions.exe error: {result.stderr.strip()}")
        except Exception as e:
            print(f"parse_sessions.exe failed, falling back to Python: {e}")

    return _parse_events_to_sessions_py()

def _parse_events_to_sessions_py() -> list:
    """Python によるフォールバック実装"""
    if not os.path.exists(EVENTS_FILE):
        return []

    events = []
    try:
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            for line in f:
                stripped = line.strip()
                if not stripped:
                    continue
                parts = stripped.split(",", 1)
                if len(parts) == 2:
                    try:
                        events.append((parse_datetime(parts[0]), parts[1]))
                    except ValueError:
                        continue
    except Exception as e:
        print(f"Error reading events file: {e}")
        return []

    if not events:
        return []

    events.sort(key=lambda x: x[0])
    MIN_SLEEP = _load_min_duration()

    sessions = []
    state = "ACTIVE"
    sleep_start = None
    session_type = None
    is_out = False

    def end_sleep(ts):
        nonlocal state, sleep_start, session_type
        dur = ts - sleep_start
        if dur >= MIN_SLEEP:
            sessions.append((
                sleep_start.strftime("%Y-%m-%d %H:%M:%S"),
                ts.strftime("%Y-%m-%d %H:%M:%S"),
                dur.total_seconds() / 3600.0,
                session_type,
            ))
        state = "ACTIVE"
        sleep_start = session_type = None

    for i, (ts, event) in enumerate(events):
        if event == "DEVICE_ON":
            if state == "SLEEPING":
                end_sleep(ts)
            continue
        elif event == "OUT_START":
            is_out = True
            if state == "SLEEPING":
                end_sleep(ts)
            continue
        elif event == "OUT_END":
            is_out = False
            continue

        if state == "ACTIVE":
            if not is_out and event in ("IDLE_START", "SUSPEND", "SHUTDOWN"):
                state = "SLEEPING"
                sleep_start = ts
                session_type = "IDLE" if event == "IDLE_START" else "POWER"
            elif event in ("STARTUP", "RESUME") and i > 0:
                if not is_out:
                    prev_ts, _ = events[i - 1]
                    if ts - prev_ts > timedelta(hours=4):
                        hb = read_last_heartbeat()
                        if hb and prev_ts < hb[0] < ts:
                            start = max(prev_ts, hb[0] - timedelta(milliseconds=hb[1]))
                        else:
                            start = prev_ts
                        dur = ts - start
                        if dur >= MIN_SLEEP:
                            sessions.append((
                                start.strftime("%Y-%m-%d %H:%M:%S"),
                                ts.strftime("%Y-%m-%d %H:%M:%S"),
                                dur.total_seconds() / 3600.0,
                                "POWER_LOSS",
                            ))
        elif state == "SLEEPING":
            if event in ("IDLE_RESUME", "RESUME", "STARTUP"):
                end_sleep(ts)
            elif event in ("SUSPEND", "SHUTDOWN"):
                session_type = "POWER"

    return sessions

def _sort_events_file():
    """sleep_events.txt をタイムスタンプ順にソートして上書きする"""
    if not os.path.exists(EVENTS_FILE):
        return
    try:
        parsed = []
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            for line in f:
                stripped = line.strip()
                if not stripped:
                    continue
                parts = stripped.split(",", 1)
                if len(parts) == 2:
                    try:
                        parsed.append((parse_datetime(parts[0]), stripped))
                    except ValueError:
                        parsed.append((datetime.min, stripped))
        parsed.sort(key=lambda x: x[0])
        with open(EVENTS_FILE, "w", encoding="utf-8") as f:
            for _, line in parsed:
                f.write(line + "\n")
    except Exception as e:
        print(f"Error sorting events file: {e}")

# ── セッション取得 ────────────────────────────────────────────────

def get_all_sessions() -> list:
    """sleep_events.txt を直接パースして睡眠セッションのリストを返す"""
    return _parse_events_to_sessions()

def get_sessions_with_ids() -> list:
    """インデックス付きセッションを返す: (index, start, end, dur, type)"""
    return [(i, s, e, d, t) for i, (s, e, d, t) in enumerate(_parse_events_to_sessions())]

# ── セッション追加・削除 ──────────────────────────────────────────

def add_session_and_rebuild(start_time_str: str, end_time_str: str, session_type: str = "sleep") -> tuple[bool, str]:
    """新しい睡眠セッションを IDLE_START/IDLE_RESUME イベントとして追加する"""
    try:
        start_dt = parse_datetime(start_time_str)
        end_dt = parse_datetime(end_time_str)
    except Exception as e:
        return False, str(e)

    if start_dt >= end_dt:
        return False, "開始時刻は終了時刻より前である必要があります。"

    for s, e, _, _ in _parse_events_to_sessions():
        if not e:
            continue
        if parse_datetime(s) < end_dt and start_dt < parse_datetime(e):
            return False, "既存の睡眠記録と時間帯が重複しています。"

    start_line = f"{start_time_str},IDLE_START"
    if os.path.exists(EVENTS_FILE):
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            if any(l.strip() == start_line for l in f):
                return False, "同じ開始時刻の記録が既に存在します。"

    try:
        os.makedirs(LOG_DIR, exist_ok=True)
        with open(EVENTS_FILE, "a", encoding="utf-8") as f:
            f.write(f"{start_time_str},IDLE_START\n")
            f.write(f"{end_time_str},IDLE_RESUME\n")
        _sort_events_file()
        git_push_logs()
        return True, "成功"
    except Exception as e:
        return False, str(e)

def delete_session_and_rebuild(session_index: int) -> bool:
    """指定インデックスの睡眠セッションに対応するイベント行を削除してプッシュする"""
    try:
        sessions = _parse_events_to_sessions()
        if session_index < 0 or session_index >= len(sessions):
            return False

        start_str, end_str, _, stype = sessions[session_index]

        if stype == "IDLE":
            remove = {f"{start_str},IDLE_START", f"{end_str},IDLE_RESUME"}
        elif stype == "POWER":
            remove = {f"{start_str},SHUTDOWN", f"{start_str},SUSPEND",
                      f"{end_str},STARTUP",  f"{end_str},RESUME"}
        else:
            return False  # POWER_LOSS はイベント行が確定できないため削除不可

        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            lines = f.readlines()
        new_lines = [l for l in lines if l.strip() not in remove]
        with open(EVENTS_FILE, "w", encoding="utf-8") as f:
            f.writelines(new_lines)

        git_push_logs()
        return True
    except Exception as e:
        print(f"Error deleting session: {e}")
        return False

def bulk_import_sessions(sessions: list) -> tuple[int, int]:
    """CSVからのセッションを IDLE_START/IDLE_RESUME イベントとして追記する。
    sessions: list of (start_str, end_str, stype)
    Returns: (imported, skipped)
    """
    existing = set()
    if os.path.exists(EVENTS_FILE):
        with open(EVENTS_FILE, "r", encoding="utf-8") as f:
            existing = {l.strip() for l in f}

    imported = skipped = 0
    new_lines = []

    for start_str, end_str, _ in sessions:
        try:
            s_dt = parse_datetime(start_str)
            e_dt = parse_datetime(end_str)
            if s_dt >= e_dt:
                skipped += 1
                continue
        except Exception:
            skipped += 1
            continue

        start_line = f"{start_str},IDLE_START"
        if start_line in existing:
            skipped += 1
            continue

        new_lines.append(start_line + "\n")
        end_line = f"{end_str},IDLE_RESUME"
        if end_line not in existing:
            new_lines.append(end_line + "\n")
        imported += 1

    if new_lines:
        os.makedirs(LOG_DIR, exist_ok=True)
        with open(EVENTS_FILE, "a", encoding="utf-8") as f:
            f.writelines(new_lines)
        _sort_events_file()
        git_push_logs()

    return imported, skipped

def clear_all_data():
    """sleep_events.txt と Gist 上のログを完全に初期化する"""
    try:
        with open(EVENTS_FILE, "w", encoding="utf-8") as f:
            f.write("")
    except Exception as e:
        print(f"Failed to clear events file: {e}")

    # Gist の mobile_event.txt を "INIT" に上書き
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
                    "User-Agent": "Sleep-Tracker-Client",
                }
                data = {"files": {"mobile_event.txt": {"content": "INIT"}}}
                req = urllib.request.Request(
                    url, data=json.dumps(data).encode("utf-8"),
                    headers=headers, method="PATCH"
                )
                with urllib.request.urlopen(req, timeout=5):
                    pass
    except Exception as e:
        print(f"Failed to reset Gist: {e}")

    try:
        git_push_logs()
    except Exception as e:
        print(f"Failed to push cleared logs to git: {e}")

# ── Gist / Git 同期 ──────────────────────────────────────────────

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

    token = github_token
    if not token:
        try:
            res = subprocess.run(
                ["gh", "auth", "token"],
                capture_output=True, text=True, check=True,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )
            token = res.stdout.strip()
        except Exception:
            pass

    if not token:
        return

    url = f"https://api.github.com/gists/{gist_id}"
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "Sleep-Tracker-Client",
    }
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            mobile_file = res_data.get("files", {}).get("mobile_event.txt", {})
            content = mobile_file.get("content", "").strip()
            if not content or content.startswith("INIT"):
                return
            parts = content.split(",", 1)
            if len(parts) == 2:
                tag = parts[0]
                if tag == "LEAVE":
                    event_type = "OUT_START"
                elif tag == "ARRIVE":
                    event_type = "OUT_END"
                elif tag == "SCREEN_ON":
                    event_type = "DEVICE_ON"
                else:
                    return
                event_time_str = parts[1].strip()
                # ミリ秒タイムスタンプ（MacroDroid等）を日時文字列に変換
                try:
                    ts_num = int(event_time_str)
                    event_time_str = datetime.fromtimestamp(ts_num / 1000.0).strftime("%Y-%m-%d %H:%M:%S")
                except ValueError:
                    pass  # すでに "YYYY-MM-DD HH:MM:SS" 形式の場合はそのまま
                new_line = f"{event_time_str},{event_type}"
                already_exists = False
                if os.path.exists(EVENTS_FILE):
                    with open(EVENTS_FILE, "r", encoding="utf-8") as ef:
                        already_exists = any(l.strip() == new_line for l in ef)
                if not already_exists:
                    with open(EVENTS_FILE, "a", encoding="utf-8") as ef:
                        ef.write(new_line + "\n")
                    print(f"Synced mobile event from Gist: {event_type} at {event_time_str}")
                if event_type == "DEVICE_ON":
                    with open(DEVICE_HEARTBEAT_FILE, "w", encoding="utf-8") as df:
                        df.write(event_time_str + "\n")
    except Exception as e:
        print(f"Failed to fetch mobile events from Gist: {e}")

def sync_with_gist():
    """Gist との双方向同期: モバイルイベント取得 + sleep_events.txt プッシュ"""
    if not os.path.exists(EVENTS_FILE):
        _download_events_from_gist()
    sync_mobile_events_from_gist()
    git_push_logs()

def validate_gist_connection():
    """Gist ID とトークンの有効性を確認する。失敗時は ValueError を送出する"""
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
    """sleep_events.txt の内容を Gist に PATCH する"""
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
            "User-Agent": "Sleep-Tracker-Client",
        }
        data = {"files": {"sleep_events.txt": {"content": content if content.strip() else " "}}}
        req = urllib.request.Request(
            url, data=json.dumps(data).encode("utf-8"), headers=headers, method="PATCH"
        )
        with urllib.request.urlopen(req, timeout=10):
            pass
        print(f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] sleep_events.txt pushed to Gist.")
    except Exception as e:
        print(f"Failed to push events to Gist: {e}")

def _download_events_from_gist():
    """Gist から sleep_events.txt をダウンロードしてローカルに書き出す"""
    gist_id, token = _get_gist_config()
    if not gist_id or not token:
        return
    try:
        url = f"https://api.github.com/gists/{gist_id}"
        headers = {
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "Sleep-Tracker-Client",
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
    """sleep_events.txt を Gist に非同期バックアップする。wait=True で最大8秒ブロック"""
    t = threading.Thread(target=_push_events_to_gist, daemon=True)
    t.start()
    if wait:
        t.join(timeout=8)
