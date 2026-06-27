// File: src_cpp/main.cpp
// Description: Windows background service to log idle time & power states.
// Date: 2026-06-27
// Author: Antigravity
// Main Functions: WinMain, WndProc, HeartbeatThread, LogEvent, GetLogPath
// Dependencies: Windows API (user32, advapi32)

#include <windows.h>
#include <string>
#include <fstream>
#include <sstream>
#include <iomanip>
#include <ctime>
#include <iostream>

// グローバル定数と変数
const wchar_t* WINDOW_CLASS_NAME = L"SleepMonitorWindowClass";
bool g_running = true;
bool g_is_idle = false;
const DWORD IDLE_THRESHOLD_MS = 20 * 60 * 1000; // 20分
std::wstring g_module_dir;

// 現在のローカル時刻を文字列(YYYY-MM-DD HH:MM:SS)で取得
std::string GetCurrentDateTimeString() {
    std::time_t now = std::time(nullptr);
    std::tm ltm;
    localtime_s(&ltm, &now);
    std::ostringstream oss;
    oss << std::put_time(&ltm, "%Y-%m-%d %H:%M:%S");
    return oss.str();
}

// 実行ファイルのあるディレクトリパスを取得する
std::wstring GetModuleDirectory() {
    wchar_t path[MAX_PATH];
    GetModuleFileName(NULL, path, MAX_PATH);
    std::wstring wpath(path);
    size_t pos = wpath.find_last_of(L"\\/");
    if (pos != std::wstring::npos) {
        return wpath.substr(0, pos + 1);
    }
    return L"";
}

// ログファイルへの書き込み処理
void LogEvent(const std::string& event_type, const std::string& timestamp = "") {
    std::wstring log_path = g_module_dir + L"sleep_events.txt";
    std::ofstream ofs(log_path, std::ios::app);
    if (ofs.is_open()) {
        std::string time_str = timestamp.empty() ? GetCurrentDateTimeString() : timestamp;
        ofs << time_str << "," << event_type << "\n";
        ofs.close();
    }
}

// ハートビートファイルへの上書き処理
void UpdateHeartbeat(DWORD idle_ms) {
    std::wstring hb_path = g_module_dir + L"sleep_heartbeat.txt";
    std::ofstream ofs(hb_path, std::ios::trunc); // 常に上書き
    if (ofs.is_open()) {
        ofs << GetCurrentDateTimeString() << "," << idle_ms << "\n";
        ofs.close();
    }
}

// アイドル時間（ミリ秒）を取得する
DWORD GetIdleTime() {
    LASTINPUTINFO lii;
    lii.cbSize = sizeof(LASTINPUTINFO);
    if (GetLastInputInfo(&lii)) {
        DWORD tick_count = GetTickCount();
        if (tick_count >= lii.dwTime) {
            return tick_count - lii.dwTime;
        } else {
            // オーバーフロー対策
            return (0xFFFFFFFF - lii.dwTime) + tick_count;
        }
    }
    return 0;
}

// ハートビート更新用スレッド関数
DWORD WINAPI HeartbeatThreadFunc(LPVOID lpParam) {
    while (g_running) {
        DWORD idle_ms = GetIdleTime();
        UpdateHeartbeat(idle_ms);

        if (idle_ms >= IDLE_THRESHOLD_MS) {
            if (!g_is_idle) {
                g_is_idle = true;
                // 最後の入力があった正確な時刻を計算（現在時刻 - アイドル時間）
                std::time_t start_time = std::time(nullptr) - (idle_ms / 1000);
                std::tm ltm;
                localtime_s(&ltm, &start_time);
                std::ostringstream oss;
                oss << std::put_time(&ltm, "%Y-%m-%d %H:%M:%S");
                LogEvent("IDLE_START", oss.str());
            }
        } else {
            if (g_is_idle) {
                g_is_idle = false;
                LogEvent("IDLE_RESUME");
            }
        }
        Sleep(60000); // 1分(60秒)待機
    }
    return 0;
}

// ウィンドウメッセージプロシージャ（電源イベント・シャットダウンの処理）
LRESULT CALLBACK WndProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
        case WM_POWERBROADCAST:
            if (wParam == PBT_APMSUSPEND) {
                LogEvent("SUSPEND");
            } else if (wParam == PBT_APMRESUMEAUTOMATIC || wParam == PBT_APMRESUMESUSPEND) {
                LogEvent("RESUME");
            }
            return TRUE;

        case WM_QUERYENDSESSION:
            // シャットダウン要求に対して「準備完了」を返す
            return TRUE;

        case WM_ENDSESSION:
            if (wParam == TRUE) { // 実際に終了する場合
                LogEvent("SHUTDOWN");
                g_running = false;
                PostQuitMessage(0);
            }
            return 0;

        case WM_DESTROY:
            LogEvent("TERMINATE");
            g_running = false;
            PostQuitMessage(0);
            return 0;
    }
    return DefWindowProc(hwnd, uMsg, wParam, lParam);
}

// エントリーポイント (Windows GUI アプリケーションとして起動)
int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow) {
    // 実行ファイルディレクトリの初期化
    g_module_dir = GetModuleDirectory();

    // 起動イベント記録
    LogEvent("STARTUP");

    // メッセージ受信用の非表示ウィンドウの登録・作成
    WNDCLASSEX wc = {0};
    wc.cbSize = sizeof(WNDCLASSEX);
    wc.lpfnWndProc = WndProc;
    wc.hInstance = hInstance;
    wc.lpszClassName = WINDOW_CLASS_NAME;

    if (!RegisterClassEx(&wc)) {
        LogEvent("ERROR_REGISTER_CLASS");
        return 1;
    }

    // HWND_MESSAGE を指定してメッセージ専用ウィンドウとして作成 (画面には映らない)
    HWND hwnd = CreateWindowEx(
        0, WINDOW_CLASS_NAME, L"Sleep Monitor Helper",
        0, 0, 0, 0, 0,
        HWND_MESSAGE, NULL, hInstance, NULL
    );

    if (!hwnd) {
        LogEvent("ERROR_CREATE_WINDOW");
        return 1;
    }

    // ハートビート更新用スレッドの起動
    HANDLE hThread = CreateThread(NULL, 0, HeartbeatThreadFunc, NULL, 0, NULL);
    if (!hThread) {
        LogEvent("ERROR_CREATE_THREAD");
        DestroyWindow(hwnd);
        return 1;
    }

    // メッセージループ
    MSG msg;
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }

    // スレッドの終了待機とクローズ
    g_running = false;
    WaitForSingleObject(hThread, 5000);
    CloseHandle(hThread);

    return (int)msg.wParam;
}
