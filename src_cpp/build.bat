@echo off
rem File: src_cpp/build.bat
rem Description: Build script for sleep_monitor.exe and parse_sessions.exe using MSVC cl or MinGW g++.
rem Date: 2026-06-27

set FAILED=0

echo Checking compilers...

where cl.exe >nul 2>nul
if %ERRORLEVEL% equ 0 (
    echo MSVC cl.exe detected.

    echo Building sleep_monitor.exe...
    cl.exe /EHsc /O2 main.cpp /link user32.lib advapi32.lib /subsystem:windows /OUT:sleep_monitor.exe
    if %ERRORLEVEL% neq 0 ( echo MSVC: sleep_monitor build failed. & set FAILED=1 )

    echo Building parse_sessions.exe...
    cl.exe /EHsc /O2 /std:c++17 parse_sessions.cpp /link /subsystem:console /OUT:parse_sessions.exe
    if %ERRORLEVEL% neq 0 ( echo MSVC: parse_sessions build failed. & set FAILED=1 )

    if %FAILED% equ 0 ( echo All builds successful. )
    exit /b %FAILED%
)

where g++.exe >nul 2>nul
if %ERRORLEVEL% equ 0 (
    echo MinGW g++.exe detected.

    echo Building sleep_monitor.exe...
    g++ -O2 main.cpp -o sleep_monitor.exe -luser32 -ladvapi32 -mwindows
    if %ERRORLEVEL% neq 0 ( echo MinGW: sleep_monitor build failed. & set FAILED=1 )

    echo Building parse_sessions.exe...
    g++ -O2 -std=c++17 -static parse_sessions.cpp -o parse_sessions.exe
    if %ERRORLEVEL% neq 0 ( echo MinGW: parse_sessions build failed. & set FAILED=1 )

    if %FAILED% equ 0 ( echo All builds successful. )
    exit /b %FAILED%
)

echo Error: Neither cl.exe nor g++.exe was found in PATH.
echo Please run this bat in Developer Command Prompt for Visual Studio, or add MinGW to your PATH.
exit /b 1
