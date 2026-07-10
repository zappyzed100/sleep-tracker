//! validation.rs — Driveから取得した内容のクライアント側検証
//!
//! 役割 : GAS側（worker/appsscript.gs）と同じ検証をクライアント側でも独立に行う。
//!        GAS自身が"error: ..."を返してくれるケースはそれで弾けるが、Googleの
//!        認証リダイレクト等GASのスクリプトロジックを経由せずに割り込まれるケース
//!        （実際に発生した、ログインページのHTMLがそのままsleep_events_backup.txtに
//!        混入した事故）はGAS側の検証をすり抜けるため、クライアント側でも中身を見て
//!        判断する必要がある。
//!
//! 依存 : なし

pub(super) fn looks_like_html_or_js(content: &str) -> bool {
    let head: String = content.chars().take(5000).collect::<String>().to_lowercase();
    ["<!doctype", "<html", "<head", "<script", "<meta", "(function()", "document.queryselector"]
        .iter()
        .any(|pat| head.contains(pat))
}

// sleep_events.txtの形式検証: 各行 "YYYY-MM-DD HH:MM:SS,TAG"。
// 非空行の90%以上が形式に一致しなければ不正とみなす（GAS側looksLikeEventsContent_と同じ基準）。
pub(super) fn looks_like_events_content(content: &str) -> bool {
    if looks_like_html_or_js(content) { return false; }
    let lines: Vec<&str> = content.lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() { return false; }
    let valid = lines.iter().filter(|l| is_event_line(l)).count();
    (valid as f64 / lines.len() as f64) >= 0.9
}

fn is_event_line(line: &str) -> bool {
    if line.len() < 21 { return false; }
    if !is_timestamp_like(&line[..19]) { return false; }
    if line.as_bytes()[19] != b',' { return false; }
    let rest = &line[20..];
    !rest.is_empty() && !rest.contains('<') && !rest.contains('>')
}

fn is_timestamp_like(ts: &str) -> bool {
    let b = ts.as_bytes();
    if b.len() != 19 { return false; }
    let d = |i: usize| b[i].is_ascii_digit();
    d(0) && d(1) && d(2) && d(3) && b[4] == b'-'
        && d(5) && d(6) && b[7] == b'-'
        && d(8) && d(9) && b[10] == b' '
        && d(11) && d(12) && b[13] == b':'
        && d(14) && d(15) && b[16] == b':'
        && d(17) && d(18)
}
