# File: src_python/lifecycle.py
# Description: Shared lifecycle synchronization helpers and paths for UI and monitor.
# Date: 2026-06-27
# Author: Antigravity
# Main Functions: ensure_monitor_running, ensure_startup_registered, check_process_exists, read_last_heartbeat, is_monitor_running
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
STARTUP_SHORTCUT_PATH = os.path.expandvars(
    r'%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\SleepTrackerMonitor.lnk'
)

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

def is_monitor_running() -> tuple[bool, str]:
    """監視サービスが稼働しているかを PID ファイルで確認し、フォールバックでハートビートを使う"""
    if os.path.exists(PID_FILE):
        try:
            with open(PID_FILE, "r") as f:
                pid = int(f.read().strip())
            if check_process_exists(pid):
                return True, f"稼働中 (PID: {pid})"
        except Exception:
            pass

    # PID ファイルがないか無効な場合はハートビートにフォールバック
    hb_info = read_last_heartbeat()
    if not hb_info:
        return False, "停止中 (生存信号なし)"
    hb_time, _ = hb_info
    if datetime.now() - hb_time < timedelta(minutes=3):
        return True, f"稼働中 (最終更新: {hb_time.strftime('%H:%M:%S')})"
    return False, f"停止中 (最終更新: {hb_time.strftime('%m-%d %H:%M')})"

def ensure_monitor_running():
    """バックグラウンド監視モニターが稼働していない場合、自動起動する (PIDファイルで判定)"""
    # PID ファイルが存在し、プロセスが生きていれば何もしない
    if os.path.exists(PID_FILE):
        try:
            with open(PID_FILE, "r") as f:
                pid = int(f.read().strip())
            if check_process_exists(pid):
                return
        except Exception:
            pass

    pythonw_exe = os.path.join(BASE_DIR, ".venv", "Scripts", "pythonw.exe")
    if not os.path.exists(pythonw_exe):
        pythonw_exe = sys.executable.replace("python.exe", "pythonw.exe")

    try:
        # PowerShell の Start-Process 経由で独立プロセスとして起動 (親終了の巻き添えを防ぐ)
        ps_cmd = f"Start-Process -WindowStyle Hidden -FilePath '{pythonw_exe}' -ArgumentList '\"{MONITOR_PATH}\"'"
        subprocess.Popen(["powershell", "-Command", ps_cmd], creationflags=subprocess.CREATE_NO_WINDOW)
        print("Auto-started background monitor.py successfully.")
    except Exception as e:
        print(f"Failed to auto-start monitor.py: {e}")

MAIN_PATH = os.path.join(BASE_DIR, "src_python", "main.py")
START_MENU_SHORTCUT_PATH = os.path.expandvars(
    r'%APPDATA%\Microsoft\Windows\Start Menu\Programs\睡眠トラッカー.lnk'
)

# C# コード: PowerShell の Add-Type でコンパイルし、.lnk に AUMID を書き込む
_CS_AUMID = (
    'using System;\n'
    'using System.Runtime.InteropServices;\n'
    '[ComImport,Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]\n'
    'public interface IPropertyStore {\n'
    '    [PreserveSig] int GetCount([Out] out uint c);\n'
    '    [PreserveSig] int GetAt(uint i, out PropertyKey k);\n'
    '    [PreserveSig] int GetValue(ref PropertyKey k, ref PropVariant v);\n'
    '    [PreserveSig] int SetValue(ref PropertyKey k, ref PropVariant v);\n'
    '    [PreserveSig] int Commit();\n'
    '}\n'
    '[StructLayout(LayoutKind.Sequential,Pack=4)]\n'
    'public struct PropertyKey { public Guid fmtid; public uint pid; }\n'
    '[StructLayout(LayoutKind.Explicit,Size=16)]\n'
    'public struct PropVariant { [FieldOffset(0)] public ushort vt; [FieldOffset(8)] public IntPtr p; }\n'
    'public class LnkAumid {\n'
    '    [DllImport("shell32.dll",CharSet=CharSet.Unicode)]\n'
    '    public static extern int SHGetPropertyStoreFromParsingName(string path,IntPtr pbc,uint mode,[In] ref Guid iid,[MarshalAs(UnmanagedType.Interface)] out IPropertyStore store);\n'
    '    public static void Set(string lnk,string id) {\n'
    '        var iid=new Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99");\n'
    '        IPropertyStore s;\n'
    '        if(SHGetPropertyStoreFromParsingName(lnk,IntPtr.Zero,2,ref iid,out s)<0||s==null)return;\n'
    '        var k=new PropertyKey{fmtid=new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"),pid=5};\n'
    '        IntPtr p=Marshal.StringToCoTaskMemUni(id);\n'
    '        var v=new PropVariant{vt=31,p=p};\n'
    '        s.SetValue(ref k,ref v);s.Commit();\n'
    '        Marshal.FreeCoTaskMem(p);Marshal.ReleaseComObject(s);\n'
    '    }\n'
    '}'
)

def _set_aumid_on_lnk(lnk_path: str, aumid: str) -> bool:
    """PowerShell + C# でショートカット (.lnk) に System.AppUserModel.ID を書き込む"""
    lnk_esc  = lnk_path.replace("'", "''")
    aumid_esc = aumid.replace("'", "''")
    # PowerShell double-quoted here-string (@"..."@) で C# をコンパイル
    ps_script = (
        'Add-Type -TypeDefinition @"\n'
        + _CS_AUMID + '\n'
        + '"@ -Language CSharp\n'
        + f"[LnkAumid]::Set('{lnk_esc}', '{aumid_esc}')\n"
    )
    try:
        result = subprocess.run(
            ["powershell", "-Command", ps_script],
            capture_output=True, text=True,
            creationflags=subprocess.CREATE_NO_WINDOW,
            timeout=20,
        )
        return result.returncode == 0
    except Exception as e:
        print(f"Failed to set AUMID on shortcut: {e}")
        return False

def register_start_menu_shortcut():
    """スタートメニューに AUMID 付きショートカットを作成し、タスクバーピン留めのアイコン・再起動を正常化する"""
    if os.path.exists(START_MENU_SHORTCUT_PATH):
        return

    pythonw_exe = os.path.join(BASE_DIR, ".venv", "Scripts", "pythonw.exe")
    if not os.path.exists(pythonw_exe):
        pythonw_exe = sys.executable.replace("python.exe", "pythonw.exe")

    ico_path = os.path.join(BASE_DIR, "src_python", "sleep_tracker.ico")
    icon_arg = f"$s.IconLocation = '{ico_path.replace(chr(39), chr(39)*2)}'" if os.path.exists(ico_path) else ""

    main_path_esc     = MAIN_PATH.replace("'", "''")
    pythonw_exe_esc   = pythonw_exe.replace("'", "''")
    shortcut_path_esc = START_MENU_SHORTCUT_PATH.replace("'", "''")
    base_dir_esc      = BASE_DIR.replace("'", "''")

    ps_create = f"""
$w = New-Object -ComObject WScript.Shell
$s = $w.CreateShortcut('{shortcut_path_esc}')
$s.TargetPath = '{pythonw_exe_esc}'
$s.Arguments = '"{main_path_esc}"'
$s.WorkingDirectory = '{base_dir_esc}'
{icon_arg}
$s.Save()
"""
    try:
        subprocess.run(
            ["powershell", "-Command", ps_create],
            capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW,
        )
    except Exception:
        return

    if os.path.exists(START_MENU_SHORTCUT_PATH):
        _set_aumid_on_lnk(START_MENU_SHORTCUT_PATH, "SleepTracker.UI.1")

def create_desktop_shortcut() -> bool:
    """デスクトップに睡眠トラッカー UI のショートカットを作成する"""
    pythonw_exe = os.path.join(BASE_DIR, ".venv", "Scripts", "pythonw.exe")
    if not os.path.exists(pythonw_exe):
        pythonw_exe = sys.executable.replace("python.exe", "pythonw.exe")

    ico_path = os.path.join(BASE_DIR, "src_python", "sleep_tracker.ico")
    icon_arg = f"$Shortcut.IconLocation = '{ico_path.replace(chr(39), chr(39)*2)}'" if os.path.exists(ico_path) else ""

    main_path_esc = MAIN_PATH.replace("'", "''")
    pythonw_exe_esc = pythonw_exe.replace("'", "''")
    base_dir_esc = BASE_DIR.replace("'", "''")

    ps_script = f"""
$WshShell = New-Object -ComObject WScript.Shell
$Desktop = $WshShell.SpecialFolders('Desktop')
$Shortcut = $WshShell.CreateShortcut("$Desktop\\睡眠トラッカー.lnk")
$Shortcut.TargetPath = '{pythonw_exe_esc}'
$Shortcut.Arguments = '"{main_path_esc}"'
$Shortcut.WorkingDirectory = '{base_dir_esc}'
{icon_arg}
$Shortcut.Save()
"""
    try:
        result = subprocess.run(
            ["powershell", "-Command", ps_script],
            capture_output=True,
            creationflags=subprocess.CREATE_NO_WINDOW
        )
        if result.returncode == 0:
            desktop = os.path.join(os.path.expanduser("~"), "Desktop")
            lnk_path = os.path.join(desktop, "睡眠トラッカー.lnk")
            if os.path.exists(lnk_path):
                _set_aumid_on_lnk(lnk_path, "SleepTracker.UI.1")
        return result.returncode == 0
    except Exception as e:
        print(f"Failed to create desktop shortcut: {e}")
        return False

def remove_startup_registration():
    """Windows スタートアップフォルダのショートカットを削除し、PC 起動時の自動実行を解除する"""
    try:
        if os.path.exists(STARTUP_SHORTCUT_PATH):
            os.remove(STARTUP_SHORTCUT_PATH)
            print(f"Startup shortcut removed: {STARTUP_SHORTCUT_PATH}")
    except Exception as e:
        print(f"Failed to remove startup shortcut: {e}")

def ensure_startup_registered():
    """Windows スタートアップフォルダへのショートカットを作成し、PC 起動時に monitor が自動実行されるよう登録する"""
    # すでに正しいショートカットが存在する場合はスキップ
    if os.path.exists(STARTUP_SHORTCUT_PATH):
        return

    pythonw_exe = os.path.join(BASE_DIR, ".venv", "Scripts", "pythonw.exe")
    if not os.path.exists(pythonw_exe):
        pythonw_exe = sys.executable.replace("python.exe", "pythonw.exe")

    ico_path = os.path.join(BASE_DIR, "src_python", "sleep_tracker.ico")
    icon_arg = f"$Shortcut.IconLocation = '{ico_path.replace(chr(39), chr(39)*2)}'" if os.path.exists(ico_path) else ""

    monitor_path_esc = MONITOR_PATH.replace("'", "''")
    pythonw_exe_esc = pythonw_exe.replace("'", "''")
    shortcut_path_esc = STARTUP_SHORTCUT_PATH.replace("'", "''")
    base_dir_esc = BASE_DIR.replace("'", "''")

    ps_script = f"""
$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut('{shortcut_path_esc}')
$Shortcut.TargetPath = '{pythonw_exe_esc}'
$Shortcut.Arguments = '"{monitor_path_esc}"'
$Shortcut.WorkingDirectory = '{base_dir_esc}'
{icon_arg}
$Shortcut.Save()
"""
    try:
        subprocess.run(
            ["powershell", "-Command", ps_script],
            capture_output=True,
            creationflags=subprocess.CREATE_NO_WINDOW
        )
        print(f"Startup shortcut registered: {STARTUP_SHORTCUT_PATH}")
    except Exception as e:
        print(f"Failed to register startup shortcut: {e}")
