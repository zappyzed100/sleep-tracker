//! manual.rs — 手動睡眠の版付き操作を解決
//!
//! 役割 : Drive の行単位 union 同期後も追加・削除の前後関係を復元し、
//!        一度削除した開始時刻への再入力を可能にする。
//!
//! 公開 : events内部向け `manual_sessions_from_str`, `manual_session_is_active`

enum ManualState { Active(String), Deleted }

pub(super) fn manual_sessions_from_str(raw: &str) -> Vec<(String, String)> {
    let mut latest: std::collections::HashMap<String, (u128, ManualState)> = std::collections::HashMap::new();
    for line in raw.lines() {
        let line = line.trim().trim_start_matches('\u{FEFF}');
        if line.is_empty() { continue; }
        let mut parts = line.split(',');
        let Some(start) = parts.next() else { continue };
        let Some(value) = parts.next() else { continue };
        let metadata = parts.next();
        let (revision, state) = if let Some(rev) = value.strip_prefix("MANUAL_DELETED:") {
            (rev.parse::<u128>().unwrap_or(1), ManualState::Deleted)
        } else if value == "MANUAL_DELETED" {
            (1, ManualState::Deleted)
        } else {
            let revision = metadata.and_then(|m| m.strip_prefix("MANUAL_REV:"))
                .and_then(|r| r.parse::<u128>().ok()).unwrap_or(0);
            (revision, ManualState::Active(value.to_string()))
        };
        let replace = latest.get(start).is_none_or(|(old_revision, old_state)| {
            revision > *old_revision || (revision == *old_revision
                && matches!(state, ManualState::Deleted)
                && !matches!(old_state, ManualState::Deleted))
        });
        if replace { latest.insert(start.to_string(), (revision, state)); }
    }
    let mut sessions = latest.into_iter().filter_map(|(start, (_, state))| match state {
        ManualState::Active(end) => Some((start, end)),
        ManualState::Deleted => None,
    }).collect::<Vec<_>>();
    sessions.sort_by(|a, b| a.0.cmp(&b.0));
    sessions
}

pub(super) fn manual_session_is_active(raw: &str, start: &str) -> bool {
    manual_sessions_from_str(raw).iter().any(|(s, _)| s == start)
}
