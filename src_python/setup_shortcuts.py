# File: src_python/setup_shortcuts.py
# Description: Script to set up Windows startup task and desktop shortcut with custom crescent moon icon.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: create_shortcut, get_pythonw_path, get_desktop_dir, create_crescent_ico, main
# Dependencies: os, sys, subprocess, winreg, PIL

import os
import sys
import subprocess
import winreg
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
    """現在アクティブな仮想環境、またはシステム上の pythonw.exe のパスを取得する"""
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
        # 32x32 のマスク画像を生成 (0: 透明, 255: 不透明)
        mask1 = Image.new("L", (32, 32), 0)
        draw1 = ImageDraw.Draw(mask1)
        draw1.ellipse((4, 4, 28, 28), fill=255) # 月の本体
        
        # 重ねる円で三日月の形状に削る
        draw_mask = ImageDraw.Draw(mask1)
        draw_mask.ellipse((12, 4, 36, 28), fill=0) # 削り
        
        # 黄色一色の画像を作成し、マスクを適用して透過三日月画像を作成
        yellow_img = Image.new("RGBA", (32, 32), (249, 226, 175, 255))
        crescent_img = Image.composite(yellow_img, Image.new("RGBA", (32, 32), (0,0,0,0)), mask1)
        
        # ICOフォーマットで保存
        crescent_img.save(ico_path, format="ICO", sizes=[(32, 32)])
        print(f"Created custom crescent icon: {os.path.basename(ico_path)}")
        return True
    except Exception as e:
        print(f"Failed to create icon file: {e}")
        return False

def main():
    print("Setting up Sleep Tracker shortcuts...")

    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    startup_dir = os.path.expandvars(r'%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup')
    desktop_dir = get_desktop_dir()

    cpp_monitor_exe = os.path.join(base_dir, "src_cpp", "sleep_monitor.exe")
    python_monitor_py = os.path.join(base_dir, "src_python", "monitor.py")
    python_main_py = os.path.join(base_dir, "src_python", "main.py")
    
    # アイコンファイルのパス決定
    ico_path = os.path.join(base_dir, "src_python", "sleep_tracker.ico")
    create_crescent_ico(ico_path)

    # 1. 監視サービスの自動起動設定 (スタートアップフォルダへ)
    startup_shortcut_path = os.path.join(startup_dir, "SleepTrackerMonitor.lnk")
    
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
        pythonw_exe = get_pythonw_path()
        create_shortcut(
            target=pythonw_exe,
            shortcut_path=startup_shortcut_path,
            arguments=f'"{python_monitor_py}"',
            working_dir=base_dir,
            icon_path=ico_path
        )

    # 2. 睡眠ログ表示UIのショートカット作成 (デスクトップへ)
    desktop_shortcut_path = os.path.join(desktop_dir, "Sleep Tracker Log.lnk")
    pythonw_exe = get_pythonw_path()

    print("Setting up UI shortcut on Desktop...")
    create_shortcut(
        target=pythonw_exe,
        shortcut_path=desktop_shortcut_path,
        arguments=f'"{python_main_py}"',
        working_dir=base_dir,
        icon_path=ico_path
    )

    # 3. 監視サービスの即時起動 (二重起動防止チェック付き)
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
                subprocess.Popen([cpp_monitor_exe], creationflags=0x08000000, cwd=os.path.dirname(cpp_monitor_exe))
            else:
                subprocess.Popen([pythonw_exe, python_monitor_py], creationflags=0x08000000, cwd=base_dir)
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
