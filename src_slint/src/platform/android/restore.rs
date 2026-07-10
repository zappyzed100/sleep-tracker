//! restore.rs — Android版「バックアップから復元」のファイルピッカー連携
//!
//! 役割 : rfd（Androidバックエンドを持たない）の代わりに、Kotlin側の
//!        ACTION_OPEN_DOCUMENTシステムファイルピッカーをJNI経由で起動し、
//!        選択されたファイルの内容をJNI経由で受け取ってevents::restore_eventsに
//!        渡す。以前の「restore.txtを手動配置してもらう」方式を置き換える。
//!
//! 依存 : crate::{core::events, ui::home, MainWindow}, super::bg, jni, ndk-context
//! 公開 : `launch_picker`,
//!        `Java_com_sleeptracker_app_MainActivity_nativeRestorePicked`（Kotlinの
//!        onActivityResult()から呼ばれるJNIエントリポイント）

use crate::core::events;
use crate::ui::home::{self, SharedState};
use crate::MainWindow;
use jni::{errors::LogErrorAndDefault, jni_sig, jni_str, objects::JObject, objects::JString, JavaVM};
use std::sync::Mutex;

// launch_picker()で保持し、後からKotlin側のコールバック(nativeRestorePicked)が
// 呼ばれた際にUI更新へ使う。ボタンを押すたびに上書きするためOnceLockではなくMutex。
static HANDLE: Mutex<Option<(slint::Weak<MainWindow>, SharedState)>> = Mutex::new(None);

// KotlinのMainActivity#launchRestorePicker()をJNI経由で呼び出し、
// ACTION_OPEN_DOCUMENTのシステムファイルピッカーを起動する。
// Slintのボタンコールバック（メインスレッド）から直接呼ばれる想定。
//
// ndk_context::android_context().context()はApplicationインスタンスを指しており
// （android-activityクレートの実装、MainActivity固有ではない）、launchRestorePicker()
// を呼べない。代わりにbg::activity()（nativeOnResume初回呼び出し時に保持した
// MainActivity自身へのグローバル参照）を使う。JavaVMの取得だけはndk_contextのままでよい。
pub fn launch_picker(weak: slint::Weak<MainWindow>, state: SharedState) {
    *HANDLE.lock().unwrap() = Some((weak.clone(), state));

    let Some(activity) = super::bg::activity() else {
        eprintln!("[app] android restore: ERROR MainActivity参照がまだ初期化されていません");
        report_launch_failure(&weak);
        return;
    };

    let ctx = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        env.call_method(activity.as_obj(), jni_str!("launchRestorePicker"), jni_sig!(() -> void), &[])?;
        Ok(())
    });

    if let Err(e) = result {
        eprintln!("[app] android restore: ERROR launchRestorePicker呼び出し失敗 {:?}", e);
        report_launch_failure(&weak);
    }
}

fn report_launch_failure(weak: &slint::Weak<MainWindow>) {
    if let Some(w) = weak.upgrade() {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        w.set_restore_message(format!("復元失敗: ファイルピッカーを起動できません ({})", now).into());
        w.set_restore_kind("error".into());
        w.set_restore_in_progress(false);
    }
}

// Kotlin側 MainActivity#onActivityResult() → nativeRestorePicked() から呼ばれる。
// contentはキャンセル・読み込み失敗時にKotlin側からnullで渡される。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sleeptracker_app_MainActivity_nativeRestorePicked<'caller>(
    mut unowned_env: jni::EnvUnowned<'caller>,
    _this: JObject<'caller>,
    content: JString<'caller>,
) {
    let picked: Option<String> = unowned_env
        .with_env(|env| -> jni::errors::Result<Option<String>> {
            if content.is_null() {
                Ok(None)
            } else {
                Ok(Some(content.try_to_string(env)?))
            }
        })
        .resolve::<LogErrorAndDefault>();

    let Some((weak, state)) = HANDLE.lock().unwrap().clone() else { return };

    let _ = slint::invoke_from_event_loop(move || {
        let Some(w) = weak.upgrade() else { return };
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        match picked {
            None => {
                w.set_restore_message("キャンセルしました".into());
                w.set_restore_kind("info".into());
            }
            Some(content) => match events::restore_events(content) {
                Ok(()) => {
                    w.set_restore_message(format!("✓ バックアップから復元しました ({})", now).into());
                    w.set_restore_kind("success".into());
                    home::refresh_all(&w, &state);
                }
                Err(e) => {
                    w.set_restore_message(format!("復元失敗: {} ({})", e, now).into());
                    w.set_restore_kind("error".into());
                }
            },
        }
        w.set_restore_in_progress(false);
    });
}
