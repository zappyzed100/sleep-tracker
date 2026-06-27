# File: src_python/setup_shortcuts.py
# Description: Script to set up Windows startup task and desktop shortcut.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: create_shortcut, get_pythonw_path, setup_all
# Dependencies: os, sys, subprocess

import os
import sys
import subprocess

def create_shortcut(target: str, shortcut_path: str, arguments: str = "", working_dir: str = "", icon_path: str = ""):
    """PowerShell を使用して Windows のショートカット (.lnk) ファイルを作成する"""
    # シングルクォーテーションのエスケープ処理
    target_esc = target.replace("'", "''")
    shortcut_path_esc = shortcut_path.replace("'", "''")
    arguments_esc = arguments.replace("'", "''")
    working_dir_esc = working_dir.replace("'", "''")
    icon_path_esc = icon_path.replace("'", "''")

    ps_script = f"""
    $WshShell = New-Object -ComObject WScript.Shell
    $Shortcut = $WshShell.CreateShortcut('{shortcut_path_esc}')
    $Shortcut.TargetPath = '{target_esc}'
    $Shortcut.Arguments = '{arguments_esc}'
    $Shortcut.WorkingDirectory = '{working_dir_esc}'
    if ('{icon_path_esc}' -ne '') {{
        $Shortcut.IconLocation = '{icon_path_esc}'
    }}
    $Shortcut.Save()
    """
    
    try:
        subprocess.run(["powershell", "-Command", ps_script], capture_output=True, check=True)
        print(f"Created shortcut: {os.path.basename(shortcut_path)}")
        return True
    except subprocess.CalledProcessError as e:
        print(f"Failed to create shortcut: {e}")
        print(e.stderr.decode('cp932', errors='ignore'))
        return False

def get_pythonw_path() -> str:
    """現在アクティブな仮想環境、またはシステム上の pythonw.exe のパスを取得する"""
    python_dir = os.path.dirname(sys.executable)
    pythonw_path = os.path.join(python_dir, "pythonw.exe")
    if os.path.exists(pythonw_path):
        return pythonw_path
    
    # 仮想環境が有効な場合で pythonw が隣にない場合はシステムのフォールバック
    return "pythonw.exe"

def get_desktop_dir() -> str:
    """Windows の特殊フォルダから正確なデスクトップパスを取得する（OneDrive対策）"""
    try:
        result = subprocess.run(
            ["powershell", "-Command", "[Environment]::GetFolderPath('Desktop')"],
            capture_output=True, text=True, check=True
        )
        path = result.stdout.strip()
        if os.path.exists(path):
            return path
    except Exception:
        pass
    # フォールバック
    return os.path.expandvars(r'%USERPROFILE%\Desktop')

def main():
    print("Setting up Sleep Tracker shortcuts...")

    # パス情報の解決
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    startup_dir = os.path.expandvars(r'%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup')
    desktop_dir = get_desktop_dir()

    cpp_monitor_exe = os.path.join(base_dir, "src_cpp", "sleep_monitor.exe")
    python_monitor_py = os.path.join(base_dir, "src_python", "monitor.py")
    python_main_py = os.path.join(base_dir, "src_python", "main.py")

    # 1. 監視サービスの自動起動設定 (スタートアップフォルダへ)
    startup_shortcut_path = os.path.join(startup_dir, "SleepTrackerMonitor.lnk")
    
    if os.path.exists(cpp_monitor_exe):
        # C++バイナリが存在する場合はそれを使用
        print("Using C++ monitor for background service...")
        create_shortcut(
            target=cpp_monitor_exe,
            shortcut_path=startup_shortcut_path,
            working_dir=os.path.dirname(cpp_monitor_exe)
        )
    else:
        # 存在しない場合は Python フォールバックを使用
        print("C++ binary not found. Setting up Python fallback monitor...")
        pythonw_exe = get_pythonw_path()
        create_shortcut(
            target=pythonw_exe,
            shortcut_path=startup_shortcut_path,
            arguments=f'"{python_monitor_py}"',
            working_dir=base_dir
        )

    # 2. 睡眠ログ表示UIのショートカット作成 (デスクトップへ)
    # これをユーザーがタスクバーにピン留めすることで、タスクバーからの即時起動が可能になります
    desktop_shortcut_path = os.path.join(desktop_dir, "Sleep Tracker Log.lnk")
    python_exe = sys.executable

    print("Setting up UI shortcut on Desktop...")
    create_shortcut(
        target=python_exe,
        shortcut_path=desktop_shortcut_path,
        arguments=f'"{python_main_py}"',
        working_dir=base_dir
    )

    print("\nSetup complete!")
    print("----------------------------------------------------------------")
    print(f"1. Background Monitor added to Startup: {startup_shortcut_path}")
    print(f"2. Log Viewer shortcut created on Desktop: {desktop_shortcut_path}")
    print("   -> Drag the desktop shortcut to your Taskbar to pin it for easy access.")
    print("----------------------------------------------------------------")

if __name__ == "__main__":
    main()
