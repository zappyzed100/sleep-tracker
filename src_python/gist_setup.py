# File: src_python/gist_setup.py
# Description: Automatically creates a GitHub Gist for mobile out-of-home events and sets up config.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: get_gh_token, create_gist, main
# Dependencies: subprocess, urllib.request, json, os

import subprocess
import urllib.request
import json
import os

# パスの設定
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONFIG_PATH = os.path.join(BASE_DIR, "config.json")

def get_gh_token() -> str | None:
    """GitHub CLI (gh) からアクセストークンを取得する"""
    try:
        res = subprocess.run(
            ["gh", "auth", "token"],
            capture_output=True, text=True, check=True
        )
        token = res.stdout.strip()
        if token:
            return token
    except subprocess.CalledProcessError:
        pass
    return None

def create_gist(token: str) -> str | None:
    """GitHub API を叩いて中継用の Gist を作成する"""
    url = "https://api.github.com/gists"
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "Sleep-Tracker-Setup"
    }
    
    # mobile_event.txt を含む空のGistを作成
    data = {
        "description": "Sleep Tracker Mobile Event Relay",
        "public": False, # プライベートGist
        "files": {
            "mobile_event.txt": {
                "content": "INIT,2026-06-27 12:00:00"
            }
        }
    }
    
    req = urllib.request.Request(
        url,
        data=json.dumps(data).encode("utf-8"),
        headers=headers,
        method="POST"
    )
    
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            return res_data.get("id")
    except Exception as e:
        print(f"Error calling GitHub Gist API: {e}")
        return None

def main():
    print("=== Sleep Tracker Mobile Relay Setup ===")
    
    # 1. トークンの取得
    token = get_gh_token()
    if not token:
        print("Error: GitHub CLI is not logged in. Please run 'gh auth login' first.")
        return

    # 2. Gistの作成
    print("Creating a private Gist on GitHub...")
    gist_id = create_gist(token)
    if not gist_id:
        print("Error: Failed to create GitHub Gist.")
        return
        
    print(f"Private Gist created successfully! Gist ID: {gist_id}")

    # 3. 設定ファイルの保存
    config = {
        "gist_id": gist_id
    }
    try:
        with open(CONFIG_PATH, "w", encoding="utf-8") as f:
            json.dump(config, f, indent=4)
        print(f"Configuration saved to {CONFIG_PATH}")
    except Exception as e:
        print(f"Error saving configuration: {e}")
        return

    # 4. iPhone向けガイドの出力
    print("\n" + "="*50)
    print("★ iPhone (iOS) ショートカット設定ガイド ★")
    print("="*50)
    print("iPhone の「ショートカット」アプリを開き、以下の2つのオートメーションを作成してください。")
    print("\n[オートメーション 1: 外出時]")
    print("  1. トリガー: 「出発: 自宅」")
    print("  2. アクション: 「URLの内容を取得」")
    print("     - URL: https://api.github.com/gists/" + gist_id)
    print("     - 方法: PATCH")
    print("     - ヘッダ:")
    print("         Authorization : Bearer " + token[:8] + "..." + token[-8:])
    print("         User-Agent : iOS-Shortcut")
    print("     - 要求本文: JSON")
    print("       - 新規フィールドの追加: 「キー: files」, 「タイプ: ディクショナリ」")
    print("         - files の中身:")
    print("           - 「キー: mobile_event.txt」, 「タイプ: ディクショナリ」")
    print("             - mobile_event.txt の中身:")
    print("               - 「キー: content」, 「タイプ: テキスト」")
    print("               - 値: LEAVE,現在の日付 (カスタムフォーマット yyyy-MM-dd HH:mm:ss)")
    
    print("\n[オートメーション 2: 帰宅時]")
    print("  1. トリガー: 「到着: 自宅」")
    print("  2. アクション: 同様の「URLの内容を取得」")
    print("     - 送信する要求本文 (JSON) の content の値のみ以下に変更:")
    print("       - 値: ARRIVE,現在の日付 (カスタムフォーマット yyyy-MM-dd HH:mm:ss)")
    print("\n※iPhone設定用の完全なToken:")
    print(token)
    print("="*50)

if __name__ == "__main__":
    main()
