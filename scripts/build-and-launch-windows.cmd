@echo off
setlocal

rem Explorerからダブルクリックして実行するWindows用エントリポイント。
rem build-windows.ps1がreleaseビルド、スタートメニューショートカットの登録と起動、
rem 既存プロセスの終了をまとめて行う。

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" -Launch
if errorlevel 1 (
    echo.
    echo ビルドまたは起動に失敗しました。
    pause
    exit /b 1
)

exit /b 0
