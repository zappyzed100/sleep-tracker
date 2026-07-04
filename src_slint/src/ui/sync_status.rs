// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// sync_status.rs — 同期中インジケーター(sync-in-progress)の一元管理
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 起動時同期・定期pull/push・手動ボタンなど、複数の同期処理が
//        重なって走ってもアイコンを回転させ続け、すべて完了して初めて
//        止める。単純なboolだと後発の同期の完了が先発の完了を追い越して
//        早期にfalseへ戻すレースがあるため、参照カウントで管理する。
//
// 依存 : crate::MainWindow
// 公開 : `begin`, `end`
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::MainWindow;
use std::sync::atomic::{AtomicI32, Ordering};

static ACTIVE: AtomicI32 = AtomicI32::new(0);

// 同期処理の開始を通知する。バックグラウンドスレッドから直接呼んでよい
// （内部でUIスレッドへディスパッチする）。他に実行中の同期がある場合は
// アイコンは既に回転中なので何もしない。
pub fn begin(weak: &slint::Weak<MainWindow>) {
    if ACTIVE.fetch_add(1, Ordering::SeqCst) == 0 {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                w.set_sync_in_progress(true);
            }
        });
    }
}

// 同期処理の終了を通知する。beginを呼んだら成功・失敗どちらの経路でも
// 必ず呼ぶこと。他に実行中の同期が残っていればアイコンは回転させ続ける。
// messageを渡すと完了メッセージ（設定タブのsync-message/kind）も更新する。
pub fn end(weak: &slint::Weak<MainWindow>, message: Option<(String, &'static str)>) {
    let remaining = ACTIVE.fetch_sub(1, Ordering::SeqCst) - 1;
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            if let Some((msg, kind)) = message {
                w.set_sync_message(msg.into());
                w.set_sync_kind(kind.into());
            }
            if remaining <= 0 {
                w.set_sync_in_progress(false);
            }
        }
    });
}
