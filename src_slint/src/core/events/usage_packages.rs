//! usage_packages.rs — 「睡眠判定に使うアプリ」の記録・許可状態管理
//!
//! 役割 : Android側のUsageStatsManagerが検知したパッケージ名をUSAGE_APP_SEEN:{pkg}
//!        としてsleep_events.txtに記録し、設定画面（PC/Android両方）でON/OFFを
//!        選べるようにする。ON/OFFの変更はUSAGE_APP_ALLOWED/DENIEDマーカーとして
//!        記録し、「最後のマーカーが勝つ」方式で解決する。
//!
//! 依存 : super::{SESSION_CACHE, EVENTS_FILE_LOCK}, super::parsing::sort_events_file, crate::core::cloud

use std::fs::OpenOptions;
use std::io::Write;

use super::{SESSION_CACHE, EVENTS_FILE_LOCK};
use super::parsing::sort_events_file;

// 明らかに「意図的な使用」の証拠にならないパッケージは初見時から既定でOFFにする
// （自アプリ自身・ランチャー・システムUI等）。それ以外は既定でON（現状維持）とし、
// ユーザーが設定画面で気づいた時に個別にOFFへ切り替えていく方式にする。
const DEFAULT_DENIED_PACKAGES: &[&str] = &[
    "com.sleeptracker.app",
    "com.miui.home",
    "com.miui.securitycenter",
    "com.android.systemui",
    "com.android.launcher",
    "com.android.launcher3",
];

#[derive(serde::Serialize, Clone)]
pub struct UsagePackageEntry {
    pub package: String,
    pub label: String,
    pub allowed: bool,
}

fn default_allowed(package: &str) -> bool {
    !DEFAULT_DENIED_PACKAGES.contains(&package)
}

// sleep_events.txtからUSAGE_APP_SEEN/ALLOWED/DENIEDを走査し、見えたことのある
// 全パッケージと現在の許可状態（最後のマーカーが勝つ）を返す。
pub fn list_usage_packages() -> Vec<UsagePackageEntry> {
    let path = crate::data_dir().join("sleep_events.txt");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    usage_packages_from_content(&content)
}

pub(super) fn usage_packages_from_content(content: &str) -> Vec<UsagePackageEntry> {
    // タイムスタンプ先頭の行を文字列ソートすれば時刻昇順になるため、
    // 最後に処理した状態がその時点の最新状態になる。
    let mut lines: Vec<&str> = content.lines().collect();
    lines.sort();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut labels: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut allowed_state: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for line in lines {
        let line = line.trim_end_matches('\r').trim().trim_start_matches('\u{FEFF}');
        let Some(c) = line.find(',') else { continue };
        let rest = &line[c + 1..];
        // USAGE_APP_SEEN:{package}|{アプリ名}。Kotlin側でPackageManagerから解決できた
        // ものだけがSEENとして記録される（アプリに紐づかないものは呼び出し元で
        // 既に除外済み、UsageReporter.kt参照）。
        if let Some(v) = rest.strip_prefix("USAGE_APP_SEEN:") {
            let (pkg, label) = v.split_once('|').unwrap_or((v, v));
            seen.insert(pkg.to_string());
            labels.insert(pkg.to_string(), label.to_string());
        } else if let Some(pkg) = rest.strip_prefix("USAGE_APP_ALLOWED:") {
            seen.insert(pkg.to_string());
            allowed_state.insert(pkg.to_string(), true);
        } else if let Some(pkg) = rest.strip_prefix("USAGE_APP_DENIED:") {
            seen.insert(pkg.to_string());
            allowed_state.insert(pkg.to_string(), false);
        }
    }

    let mut result: Vec<UsagePackageEntry> = seen.into_iter()
        .map(|pkg| {
            let allowed = allowed_state.get(&pkg).copied().unwrap_or_else(|| default_allowed(&pkg));
            let label = labels.get(&pkg).cloned().unwrap_or_else(|| pkg.clone());
            UsagePackageEntry { package: pkg, label, allowed }
        })
        .collect();
    // チェック済み（睡眠判定に使う）ものを先頭に、それぞれの中ではアプリ名で
    // ソートする。一覧が長くなっても、使う設定にしたアプリが表示件数制限で
    // 埋もれないようにするため。
    result.sort_by(|a, b| (!a.allowed, &a.label).cmp(&(!b.allowed, &b.label)));
    result
}

// 設定画面のトグルから呼ぶ。ON/OFFのマーカーを追記し、即座にDriveへも反映する
// （set_day_excludedと同じパターン）。
pub fn set_usage_package_allowed(package: &str, allowed: bool) -> Result<(), String> {
    let events_path = crate::data_dir().join("sleep_events.txt");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let tag = if allowed { "USAGE_APP_ALLOWED" } else { "USAGE_APP_DENIED" };
    let line = format!("{},{}:{}\n", now, tag, package);
    let mut f = OpenOptions::new().create(true).append(true).open(&events_path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    drop(f);
    sort_events_file(&events_path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    let ep = events_path.clone();
    std::thread::spawn(move || { crate::core::cloud::auto_backup_after_event(&ep); });
    Ok(())
}

// Kotlin側（UsageReporter, JNI経由）がPackageManagerでアプリ名を解決できた新規
// パッケージを検知した際に呼ぶ（アプリに紐づかないものは呼び出し元で除外済み）。
// 初見のみ1行追記する（既にSEEN/ALLOWED/DENIEDのいずれかがあれば何もしない）。
pub fn record_usage_package_seen(package: &str, label: &str) -> Result<(), String> {
    let events_path = crate::data_dir().join("sleep_events.txt");
    let _lock = EVENTS_FILE_LOCK.lock().unwrap();
    let content = std::fs::read_to_string(&events_path).unwrap_or_default();
    let already_seen = usage_packages_from_content(&content).iter().any(|e| e.package == package);
    if already_seen {
        return Ok(());
    }
    // "|"と","はワイヤーフォーマットの区切りに使っているため、アプリ名に万一
    // 含まれていても壊れないよう置換しておく（改行も同様）。
    let safe_label = label.replace(['|', ',', '\n', '\r'], " ");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("{},USAGE_APP_SEEN:{}|{}\n", now, package, safe_label);
    let mut f = OpenOptions::new().create(true).append(true).open(&events_path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    drop(f);
    drop(_lock);
    sort_events_file(&events_path)?;
    *SESSION_CACHE.lock().unwrap() = None;
    let ep = events_path.clone();
    std::thread::spawn(move || { crate::core::cloud::auto_backup_after_event(&ep); });
    Ok(())
}
