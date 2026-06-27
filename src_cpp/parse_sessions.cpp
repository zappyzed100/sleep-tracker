// File: src_cpp/parse_sessions.cpp
// Description: Parses sleep_events.txt via state machine and outputs sleep sessions as JSON to stdout.
//              Called by database.py: parse_sessions.exe <events_file> <heartbeat_file> <config_file>
// Date: 2026-06-27
// Author: Antigravity

#include <windows.h>
#include <string>
#include <vector>
#include <fstream>
#include <sstream>
#include <algorithm>
#include <iostream>
#include <iomanip>
#include <ctime>

struct Event {
    time_t epoch;
    std::string ts_str;
    std::string type;
};

struct Session {
    std::string start;
    std::string end;
    double duration_hours;
    std::string type;
};

// "YYYY-MM-DD HH:MM:SS" → time_t (ローカル時刻)
bool parse_datetime(const std::string& s, time_t& out) {
    std::tm tm = {};
    std::istringstream ss(s);
    ss >> std::get_time(&tm, "%Y-%m-%d %H:%M:%S");
    if (ss.fail()) return false;
    tm.tm_isdst = -1;
    out = mktime(&tm);
    return out != (time_t)-1;
}

// time_t → "YYYY-MM-DD HH:MM:SS"
std::string format_datetime(time_t t) {
    std::tm tm = {};
    localtime_s(&tm, &t);
    std::ostringstream oss;
    oss << std::put_time(&tm, "%Y-%m-%d %H:%M:%S");
    return oss.str();
}

// JSON 文字列エスケープ
std::string jstr(const std::string& s) {
    std::string out = "\"";
    for (char c : s) {
        if      (c == '"')  out += "\\\"";
        else if (c == '\\') out += "\\\\";
        else                out += c;
    }
    return out + "\"";
}

// 簡易 JSON パーサー: キーに対応する値を文字列で返す (フラット JSON のみ対応)
std::string json_get(const std::string& json, const std::string& key) {
    std::string needle = "\"" + key + "\"";
    size_t pos = json.find(needle);
    if (pos == std::string::npos) return "";
    pos = json.find(':', pos + needle.size());
    if (pos == std::string::npos) return "";
    ++pos;
    while (pos < json.size() && (json[pos] == ' ' || json[pos] == '\t' ||
                                  json[pos] == '\n' || json[pos] == '\r')) ++pos;
    if (pos >= json.size()) return "";
    std::string val;
    if (json[pos] == '"') {
        ++pos;
        while (pos < json.size() && json[pos] != '"') val += json[pos++];
    } else {
        while (pos < json.size() && json[pos] != ',' && json[pos] != '}' &&
               json[pos] != '\n' && json[pos] != '\r') val += json[pos++];
        while (!val.empty() && (val.back() == ' ' || val.back() == '\t')) val.pop_back();
    }
    return val;
}

std::string slurp(const std::string& path) {
    std::ifstream f(path);
    if (!f) return "";
    return std::string(std::istreambuf_iterator<char>(f), std::istreambuf_iterator<char>());
}

int main(int argc, char* argv[]) {
    if (argc < 4) {
        std::cerr << "Usage: parse_sessions.exe <events_file> <heartbeat_file> <config_file>\n";
        return 1;
    }
    std::string events_path   = argv[1];
    std::string heartbeat_path = argv[2];
    std::string config_path   = argv[3];

    // config.json から最小睡眠時間を読み込む
    int min_sleep_minutes = 30;
    {
        std::string cfg = slurp(config_path);
        if (!cfg.empty()) {
            std::string val = json_get(cfg, "idle_threshold_minutes");
            if (!val.empty()) {
                try { min_sleep_minutes = std::max(1, std::stoi(val)); } catch (...) {}
            }
        }
    }
    double min_sleep_secs = min_sleep_minutes * 60.0;

    // sleep_heartbeat.txt を読み込む (POWER_LOSS 補正用)
    time_t hb_epoch  = 0;
    long long hb_idle_ms = 0;
    {
        std::ifstream f(heartbeat_path);
        if (f) {
            std::string line;
            if (std::getline(f, line)) {
                if (!line.empty() && line.back() == '\r') line.pop_back();
                size_t comma = line.find(',');
                if (comma != std::string::npos) {
                    time_t ep;
                    if (parse_datetime(line.substr(0, comma), ep)) {
                        hb_epoch = ep;
                        try { hb_idle_ms = std::stoll(line.substr(comma + 1)); } catch (...) {}
                    }
                }
            }
        }
    }

    // sleep_events.txt を読み込む
    std::vector<Event> events;
    {
        std::ifstream f(events_path);
        if (!f) {
            std::cout << "[]" << std::endl;
            return 0;
        }
        std::string line;
        while (std::getline(f, line)) {
            if (!line.empty() && line.back() == '\r') line.pop_back();
            if (line.empty()) continue;
            size_t comma = line.find(',');
            if (comma == std::string::npos) continue;
            Event ev;
            ev.ts_str = line.substr(0, comma);
            ev.type   = line.substr(comma + 1);
            if (!parse_datetime(ev.ts_str, ev.epoch)) continue;
            events.push_back(ev);
        }
    }

    std::sort(events.begin(), events.end(), [](const Event& a, const Event& b) {
        return a.epoch < b.epoch;
    });

    // 状態遷移マシン
    std::vector<Session> sessions;
    std::string state = "ACTIVE";
    time_t sleep_start_epoch = 0;
    std::string sleep_start_str;
    std::string session_type;
    bool is_out = false;

    for (size_t i = 0; i < events.size(); ++i) {
        const Event& ev = events[i];

        if (ev.type == "OUT_START") {
            is_out = true;
            if (state == "SLEEPING") {
                double dur = difftime(ev.epoch, sleep_start_epoch);
                if (dur >= min_sleep_secs)
                    sessions.push_back({sleep_start_str, ev.ts_str, dur / 3600.0, session_type});
                state = "ACTIVE";
                sleep_start_epoch = 0;
                sleep_start_str.clear();
                session_type.clear();
            }
            continue;
        }
        if (ev.type == "OUT_END") { is_out = false; continue; }

        if (state == "ACTIVE") {
            if (!is_out && (ev.type == "IDLE_START" || ev.type == "SUSPEND" || ev.type == "SHUTDOWN")) {
                state = "SLEEPING";
                sleep_start_epoch = ev.epoch;
                sleep_start_str   = ev.ts_str;
                session_type      = (ev.type == "IDLE_START") ? "IDLE" : "POWER";

            } else if ((ev.type == "STARTUP" || ev.type == "RESUME") && i > 0) {
                if (!is_out) {
                    double gap = difftime(ev.epoch, events[i - 1].epoch);
                    if (gap > 4.0 * 3600.0) {
                        time_t start_epoch = events[i - 1].epoch;
                        std::string start_str = events[i - 1].ts_str;

                        if (hb_epoch > 0 && hb_epoch > events[i-1].epoch && hb_epoch < ev.epoch) {
                            time_t adjusted = hb_epoch - (hb_idle_ms / 1000);
                            if (adjusted > events[i-1].epoch) {
                                start_epoch = adjusted;
                                start_str   = format_datetime(adjusted);
                            }
                        }
                        double dur = difftime(ev.epoch, start_epoch);
                        if (dur >= min_sleep_secs)
                            sessions.push_back({start_str, ev.ts_str, dur / 3600.0, "POWER_LOSS"});
                    }
                }
            }

        } else if (state == "SLEEPING") {
            if (ev.type == "IDLE_RESUME" || ev.type == "RESUME" || ev.type == "STARTUP") {
                double dur = difftime(ev.epoch, sleep_start_epoch);
                if (dur >= min_sleep_secs)
                    sessions.push_back({sleep_start_str, ev.ts_str, dur / 3600.0, session_type});
                state = "ACTIVE";
                sleep_start_epoch = 0;
                sleep_start_str.clear();
                session_type.clear();

            } else if (ev.type == "SUSPEND" || ev.type == "SHUTDOWN") {
                session_type = "POWER";
            }
        }
    }

    // JSON 出力
    std::cout << "[" << std::endl;
    for (size_t i = 0; i < sessions.size(); ++i) {
        const Session& s = sessions[i];
        std::cout << "  {"
                  << "\"start\":"    << jstr(s.start)    << ","
                  << "\"end\":"      << jstr(s.end)       << ","
                  << "\"duration\":" << std::fixed << std::setprecision(10) << s.duration_hours << ","
                  << "\"type\":"     << jstr(s.type)
                  << "}";
        if (i + 1 < sessions.size()) std::cout << ",";
        std::cout << "\n";
    }
    std::cout << "]" << std::endl;

    return 0;
}
