# File: src_python/setup_shortcuts.py
# Description: Script to set up Windows startup task and desktop shortcut, with automatic PID-based safe process termination and custom crescent icon.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: create_shortcut, get_pythonw_path, get_desktop_dir, create_crescent_ico, kill_old_processes, main
# Dependencies: os, sys, subprocess, winreg, json, time, PIL

import os
import sys
import subprocess
import winreg
import json
import time
from PIL import Image, ImageDraw

def create_shortcut(target: str, shortcut_path: str, arguments: str = "", working_dir: str = "", icon_path: str = ""):
    """PowerShell を使用して Windows のショートカット (.lnk) ファイルを作成する"""
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
    """プロジェクトの仮想環境 (.venv) 内の pythonw.exe、またはシステム上の pythonw.exe のパスを取得する"""
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    # .venv/Scripts/pythonw.exe を最優先で探索 (Windows)
    venv_pythonw = os.path.join(base_dir, ".venv", "Scripts", "pythonw.exe")
    if os.path.exists(venv_pythonw):
        return venv_pythonw
        
    python_dir = os.path.dirname(sys.executable)
    pythonw_path = os.path.join(python_dir, "pythonw.exe")
    if os.path.exists(pythonw_path):
        return pythonw_path
    return "pythonw.exe"

def get_desktop_dir() -> str:
    """Windows レジストリから正確なデスクトップパスを取得する（OneDrive対策）"""
    try:
        key = winreg.OpenKey(
            winreg.HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders"
        )
        path, _ = winreg.QueryValueEx(key, "Desktop")
        winreg.CloseKey(key)
        expanded_path = os.path.expandvars(path)
        if os.path.exists(expanded_path):
            return expanded_path
    except Exception:
        pass
    return os.path.expandvars(r'%USERPROFILE%\Desktop')

def create_crescent_ico(ico_path: str):
    """完全に透過された黄色の三日月アイコンファイル (.ico) を生成する"""
    try:
        mask1 = Image.new("L", (32, 32), 0)
        draw1 = ImageDraw.Draw(mask1)
        draw1.ellipse((4, 4, 28, 28), fill=255) # 月の本体
        
        draw_mask = ImageDraw.Draw(mask1)
        draw_mask.ellipse((12, 4, 36, 28), fill=0) # 削り
        
        yellow_img = Image.new("RGBA", (32, 32), (249, 226, 175, 255))
        crescent_img = Image.composite(yellow_img, Image.new("RGBA", (32, 32), (0,0,0,0)), mask1)
        
        crescent_img.save(ico_path, format="ICO", sizes=[(32, 32)])
        print(f"Created custom crescent icon: {os.path.basename(ico_path)}")
        return True
    except Exception as e:
        print(f"Failed to create icon file: {e}")
        return False

def kill_old_processes():
    """現在動いている古い Sleep Tracker プロセスを PID（プロセスID）指定で安全に狙い撃ちキルする"""
    my_pid = os.getpid()
    print("Checking for active Sleep Tracker processes...")
    
    ps_cmd = (
        "Get-CimInstance Win32_Process -Filter 'CommandLine Like ''%sleep-tracker%''' | "
        "Select-Object ProcessId, CommandLine | ConvertTo-Json"
    )
    try:
        res = subprocess.run(
            ["powershell", "-Command", ps_cmd], 
            capture_output=True, text=True, encoding="utf-8", errors="ignore"
        )
        output = res.stdout.strip()
        if not output:
            print("No old Sleep Tracker processes found running.")
            return
            
        data = json.loads(output)
        if isinstance(data, dict):
            processes = [data]
        elif isinstance(data, list):
            processes = data
        else:
            processes = []
            
        pids_to_kill = []
        for proc in processes:
            pid = proc.get("ProcessId")
            cmdline = proc.get("CommandLine") or ""
            
            if not pid or pid == my_pid:
                continue
                
            # monitor.py または main.py を含んでいるか確認
            if "monitor.py" in cmdline or "main.py" in cmdline:
                label = "monitor.py" if "monitor.py" in cmdline else "main.py"
                print(f"  -> Found running task: PID {pid} ({label})")
                pids_to_kill.append(pid)
                
        if pids_to_kill:
            print(f"Terminating target PIDs: {pids_to_kill} ...")
            kill_cmd = ["taskkill", "/F"]
            for pid in pids_to_kill:
                kill_cmd.extend(["/PID", str(pid)])
            try:
                subprocess.run(kill_cmd, capture_output=True, check=True)
                print("Successfully terminated target processes.")
                time.sleep(1.0)
            except Exception as e:
                print(f"Failed to terminate processes: {e}")
        else:
            print("No old Sleep Tracker processes found running.")
            
    except Exception as e:
        print(f"Failed to check/terminate processes: {e}")

def main():
    # 処理の最初に古いプロセスを安全にキルする
    kill_old_processes()
    
    print("\nSetting up Sleep Tracker shortcuts...")

    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    startup_dir = os.path.expandvars(r'%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup')
    desktop_dir = get_desktop_dir()

    cpp_monitor_exe = os.path.join(base_dir, "src_cpp", "sleep_monitor.exe")
    python_monitor_py = os.path.join(base_dir, "src_python", "monitor.py")
    python_main_py = os.path.join(base_dir, "src_python", "main.py")
    
    ico_path = os.path.join(base_dir, "src_python", "sleep_tracker.ico")
    create_crescent_ico(ico_path)

    # 1. バックグラウンドサービスの自動起動設定 (スタートアップフォルダへ)
    startup_shortcut_path = os.path.join(startup_dir, "SleepTrackerMonitor.lnk")
    
    pythonw_exe = get_pythonw_path()
    if os.path.exists(cpp_monitor_exe):
        print("Using C++ monitor for background service...")
        create_shortcut(
            target=cpp_monitor_exe,
            shortcut_path=startup_shortcut_path,
            working_dir=os.path.dirname(cpp_monitor_exe),
            icon_path=ico_path
        )
    else:
        print("C++ binary not found. Setting up Python fallback monitor...")
        create_shortcut(
            target=pythonw_exe,
            shortcut_path=startup_shortcut_path,
            arguments=f'"{python_monitor_py}"',
            working_dir=base_dir,
            icon_path=ico_path
        )

    # 2. 睡眠ログ表示UIのショートカット作成 (デスクトップへ)
    desktop_shortcut_path = os.path.join(desktop_dir, "Sleep Tracker Log.lnk")

    print("Setting up UI shortcut on Desktop...")
    create_shortcut(
        target=pythonw_exe,
        shortcut_path=desktop_shortcut_path,
        arguments=f'"{python_main_py}"',
        working_dir=base_dir,
        icon_path=ico_path
    )

    # 3. バックグラウンドサービスの即時起動 (二重起動防止チェック付き)
    is_running = False
    try:
        res = subprocess.run(["tasklist"], capture_output=True, text=True, check=True)
        if "sleep_monitor.exe" in res.stdout or "monitor.py" in res.stdout:
            is_running = True
    except Exception:
        pass
        
    if not is_running:
        print("Starting background monitor service now...")
        try:
            if os.path.exists(cpp_monitor_exe):
                ps_start = f"Start-Process -WindowStyle Hidden -FilePath '{cpp_monitor_exe}' -WorkingDirectory '{os.path.dirname(cpp_monitor_exe)}'"
                subprocess.run(["powershell", "-Command", ps_start], capture_output=True, check=True)
            else:
                python_exe = get_pythonw_path().replace("pythonw.exe", "python.exe")
                ps_start = f"Start-Process -WindowStyle Hidden -FilePath '{python_exe}' -ArgumentList '\"{python_monitor_py}\"' -WorkingDirectory '{base_dir}'"
                subprocess.run(["powershell", "-Command", ps_start], capture_output=True, check=True)
            print("Background monitor service started successfully.")
        except Exception as e:
            print(f"Failed to start background monitor service: {e}")
    else:
        print("Background monitor service is already running.")

    print("\nSetup complete!")
    print("----------------------------------------------------------------")
    print(f"1. Background Monitor added to Startup: {startup_shortcut_path}")
    print(f"2. Log Viewer shortcut created on Desktop: {desktop_shortcut_path}")
    print("   -> Drag the desktop shortcut to your Taskbar to pin it for easy access.")
    print("----------------------------------------------------------------")

if __name__ == "__main__":
    main()
