@echo off
rem File: src_cpp/build.bat
rem Description: Build script for sleep_monitor.exe using MSVC cl or MinGW g++.
rem Date: 2026-06-27

echo Checking compilers...

where cl.exe >nul 2>nul
if %ERRORLEVEL% equ 0 (
    echo MSVC cl.exe detected. Building with MSVC...
    cl.exe /EHsc /O2 main.cpp /link user32.lib advapi32.lib /OUT:sleep_monitor.exe
    if %ERRORLEVEL% equ 0 (
        echo Build successful: sleep_monitor.exe
        exit /b 0
    ) else (
        echo MSVC build failed.
    )
)

where g++.exe >nul 2>nul
if %ERRORLEVEL% equ 0 (
    echo MinGW g++.exe detected. Building with MinGW...
    g++ -O2 main.cpp -o sleep_monitor.exe -luser32 -ladvapi32 -mwindows
    if %ERRORLEVEL% equ 0 (
        echo Build successful: sleep_monitor.exe
        exit /b 0
    ) else (
        echo MinGW build failed.
    )
)

echo Error: Neither cl.exe nor g++.exe was found in PATH.
echo Please run this bat in Developer Command Prompt for Visual Studio, or add MinGW to your PATH.
exit /b 1
