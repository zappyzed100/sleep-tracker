//! mcp.rs — 起動中のアプリから睡眠データを返すローカル MCP サーバー
//!
//! 役割 : Streamable HTTP の最小実装を `127.0.0.1` のみに公開し、MCP クライアントから
//!        `get_sleep_data` を呼び出せるようにする。サーバーはアプリプロセス内の
//!        スレッドとして動作するため、アプリ終了時には自動的に利用できなくなる。
//!
//! 依存 : `crate::core::events`, `serde_json`
//! 公開 : `start`, `MCP_PORT`

use crate::core::events::{self, Session};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

const TAG: &str = "[mcp]";
const MCP_PATH: &str = "/mcp";
pub const MCP_PORT: u16 = 32123;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const PROTOCOL_VERSION: &str = "2025-06-18";

static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

/// アプリ起動中だけ待ち受けるローカル MCP サーバーを開始する。
pub fn start() {
    let address = format!("127.0.0.1:{}", MCP_PORT);
    let listener = match TcpListener::bind(&address) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("{} start: ERROR {}: {}", TAG, address, error);
            return;
        }
    };

    eprintln!(
        "{} start: listening on http://{}/{}",
        TAG,
        address,
        MCP_PATH.trim_start_matches('/')
    );
    thread::spawn(move || serve(listener));
}

fn serve(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle_connection(stream));
            }
            Err(error) => eprintln!("{} accept: ERROR {}", TAG, error),
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    let request_number = REQUEST_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("{} request #{}: ERROR {}", TAG, request_number, error);
            let _ = write_http_error(&mut stream, 400, &error);
            return;
        }
    };

    if request.method != "POST" {
        let _ = write_http_error(&mut stream, 405, "MCP endpoint requires POST");
        return;
    }
    if request.path != MCP_PATH {
        let _ = write_http_error(&mut stream, 404, "not found");
        return;
    }
    if let Some(origin) = request.origin.as_deref() {
        if !is_local_origin(origin) {
            let _ = write_http_error(&mut stream, 403, "Origin is not allowed");
            return;
        }
    }

    let payload = match serde_json::from_slice::<Value>(&request.body) {
        Ok(payload) => payload,
        Err(error) => {
            let response = json_rpc_error(Value::Null, -32700, &format!("Parse error: {}", error));
            let _ = write_http_json(&mut stream, 200, &response);
            return;
        }
    };

    match dispatch(&payload) {
        Some(response) => {
            let _ = write_http_json(&mut stream, 200, &response);
        }
        None => {
            let _ = write_http_status(&mut stream, 202, "Accepted", &[]);
        }
    }
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
    origin: Option<String>,
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::with_capacity(4096);
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("接続が途中で閉じられました".to_string());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err("HTTPヘッダーが大きすぎます".to_string());
        }
    };

    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or("HTTPリクエスト行がありません")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or("HTTPメソッドがありません")?
        .to_string();
    let path = request_parts
        .next()
        .ok_or("HTTPパスがありません")?
        .to_string();

    let mut content_length = None;
    let mut origin = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Content-Lengthが不正です")?,
            );
        }
        if name.eq_ignore_ascii_case("origin") {
            origin = Some(value.trim().to_string());
        }
    }
    let content_length = match content_length {
        Some(length) => length,
        None if method == "GET" => 0,
        None => return Err("Content-Lengthがありません".to_string()),
    };
    if content_length > MAX_BODY_BYTES {
        return Err("HTTPボディが大きすぎます".to_string());
    }

    let required_size = header_end + content_length;
    while bytes.len() < required_size {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("HTTPボディが途中で終わりました".to_string());
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    Ok(HttpRequest {
        method,
        path,
        body: bytes[header_end..required_size].to_vec(),
        origin,
    })
}

fn write_http_json(stream: &mut TcpStream, status: u16, body: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(body).map_err(|error| error.to_string())?;
    write_http_status(
        stream,
        status,
        if status == 200 { "OK" } else { "Error" },
        &bytes,
    )
}

fn write_http_error(stream: &mut TcpStream, status: u16, message: &str) -> Result<(), String> {
    write_http_status(stream, status, "Error", message.as_bytes())
}

fn write_http_status(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
) -> Result<(), String> {
    let content_type = if status == 200 {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        content_type,
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())
}

fn is_local_origin(origin: &str) -> bool {
    let origin = origin.trim().to_ascii_lowercase();
    [
        "http://localhost",
        "https://localhost",
        "http://127.0.0.1",
        "https://127.0.0.1",
    ]
    .iter()
    .any(|prefix| {
        origin
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
    })
}

fn dispatch(request: &Value) -> Option<Value> {
    let Some(object) = request.as_object() else {
        return Some(json_rpc_error(Value::Null, -32600, "Invalid Request"));
    };
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let has_id = object.contains_key("id");
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return if has_id {
            Some(json_rpc_error(id, -32600, "Invalid Request"))
        } else {
            None
        };
    };
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => response_or_none(id, has_id, initialize_result(&params)),
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => response_or_none(id, has_id, json!({})),
        "tools/list" => response_or_none(id, has_id, json!({ "tools": tool_definitions() })),
        "tools/call" => match call_tool(&params) {
            Ok(result) => response_or_none(id, has_id, result),
            Err(error) => {
                if has_id {
                    Some(json_rpc_error(id, -32602, &error))
                } else {
                    None
                }
            }
        },
        _ => {
            if has_id {
                Some(json_rpc_error(id, -32601, "Method not found"))
            } else {
                None
            }
        }
    }
}

fn response_or_none(id: Value, has_id: bool, result: Value) -> Option<Value> {
    if has_id {
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    } else {
        None
    }
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    let protocol_version = match requested {
        "2024-11-05" | "2025-03-26" | "2025-06-18" => requested,
        _ => PROTOCOL_VERSION,
    };
    json!({
        "protocolVersion": protocol_version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "sleep-tracker", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "睡眠セッションと集計値を取得できます。データはアプリ起動中のローカル状態です。"
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![json!({
        "name": "get_sleep_data",
        "description": "現在の睡眠セッション、集計値、進行中の睡眠開始時刻を取得します。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "from_date": { "type": "string", "description": "開始日（YYYY-MM-DD、含む）" },
                "to_date": { "type": "string", "description": "終了日（YYYY-MM-DD、含む）" },
                "include_excluded": { "type": "boolean", "description": "計測対象外に設定した日も含めるか（既定値 false）" }
            },
            "additionalProperties": false
        }
    })]
}

fn call_tool(params: &Value) -> Result<Value, String> {
    let object = params.as_object().ok_or("tools/callのparamsが不正です")?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or("ツール名がありません")?;
    if name != "get_sleep_data" {
        return Ok(tool_error(format!("未知のツールです: {}", name)));
    }

    let arguments = match object.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        Some(_) => return Ok(tool_error("argumentsが不正です".to_string())),
    };
    let from_date = date_argument(&arguments, "from_date")?;
    let to_date = date_argument(&arguments, "to_date")?;
    if let (Some(from), Some(to)) = (&from_date, &to_date) {
        if from > to {
            return Ok(tool_error(
                "from_dateはto_date以前にしてください".to_string(),
            ));
        }
    }
    let include_excluded = arguments
        .get("include_excluded")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let sessions = events::get_sessions()?;
    let sessions = filter_sessions(
        sessions,
        from_date.as_deref(),
        to_date.as_deref(),
        include_excluded,
    );
    let current_sleep_start = events::current_sleep_start();
    let total_duration_hours: f64 = sessions.iter().map(|session| session.duration_hours).sum();
    let session_count = sessions.len();
    let average_duration_hours = if sessions.is_empty() {
        0.0
    } else {
        total_duration_hours / session_count as f64
    };
    let latest_session = sessions.last().cloned();
    let data = json!({
        "sessions": sessions,
        "summary": {
            "session_count": session_count,
            "total_duration_hours": total_duration_hours,
            "average_duration_hours": average_duration_hours,
            "latest_session": latest_session
        },
        "current_sleep_start": current_sleep_start,
        "generated_at": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
    });
    let text = serde_json::to_string_pretty(&data).map_err(|error| error.to_string())?;
    Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
}

fn date_argument(arguments: &Map<String, Value>, name: &str) -> Result<Option<String>, String> {
    let Some(value) = arguments.get(name) else {
        return Ok(None);
    };
    let Some(date) = value.as_str() else {
        return Err(format!("{}は文字列で指定してください", name));
    };
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| format!("{}はYYYY-MM-DDで指定してください", name))?;
    Ok(Some(date.to_string()))
}

fn filter_sessions(
    sessions: Vec<Session>,
    from_date: Option<&str>,
    to_date: Option<&str>,
    include_excluded: bool,
) -> Vec<Session> {
    sessions
        .into_iter()
        .filter(|session| {
            let date = session_sleep_day(session);
            (include_excluded || !session.excluded)
                && from_date.map_or(true, |from| date.as_str() >= from)
                && to_date.map_or(true, |to| date.as_str() <= to)
        })
        .collect()
}

fn session_sleep_day(session: &Session) -> String {
    chrono::NaiveDateTime::parse_from_str(&session.start, "%Y-%m-%d %H:%M:%S")
        .map(crate::core::utils::sleep_day)
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| session.start.get(..10).unwrap_or("").to_string())
}

fn tool_error(message: String) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(start: &str, excluded: bool) -> Session {
        Session {
            start: start.to_string(),
            end: format!("{} 07:00:00", &start[..10]),
            duration_hours: 7.0,
            session_type: "IDLE".to_string(),
            excluded,
        }
    }

    #[test]
    fn initialize_negotiates_supported_protocol_version() {
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2024-11-05" }
        });
        let response = dispatch(&request).expect("request response");
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "sleep-tracker");
    }

    #[test]
    fn tools_list_exposes_sleep_data_tool() {
        let response = dispatch(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .expect("request response");
        assert_eq!(response["result"]["tools"][0]["name"], "get_sleep_data");
    }

    #[test]
    fn notifications_do_not_get_a_response() {
        assert!(
            dispatch(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none()
        );
    }

    #[test]
    fn filter_sessions_applies_date_and_excluded_filters() {
        let sessions = vec![
            session("2026-08-01 23:00:00", false),
            session("2026-08-02 23:00:00", true),
        ];
        let filtered = filter_sessions(sessions, Some("2026-08-01"), Some("2026-08-02"), false);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].start, "2026-08-01 23:00:00");
    }

    #[test]
    fn date_filter_uses_the_app_sleep_day_boundary() {
        let sessions = vec![session("2026-08-02 01:00:00", false)];
        let filtered = filter_sessions(sessions, Some("2026-08-01"), Some("2026-08-01"), false);
        assert_eq!(filtered.len(), 1);
    }
}
