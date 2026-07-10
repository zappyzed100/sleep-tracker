//! usage.rs — 「睡眠判定に使うアプリ」のKotlin↔Rust JNIブリッジ
//!
//! 役割 : UsageReporter.kt（Android実機のUsageStatsManager監視）が検知した
//!        パッケージ名の記録・現在の許可状態の取得をRust側（core::events）に
//!        橋渡しする。データの実体・設定画面からのON/OFF切り替えはすべて
//!        core::events側に集約し、ここはJNIの型変換のみを担当する。
//!
//! 依存 : crate::core::events, super::bg, jni
//! 公開 : `Java_com_sleeptracker_app_UsageReporter_nativeRecordUsagePackageSeen`,
//!        `Java_com_sleeptracker_app_UsageReporter_nativeGetDeniedPackages`,
//!        `Java_com_sleeptracker_app_UsageReporter_nativeReportUsageScanComplete`

use crate::core::events;
use jni::errors::LogErrorAndDefault;
use jni::objects::{JObject, JString};
use jni::EnvUnowned;

// UsageReporterはKotlinの`object`（シングルトン）なので、external funはインスタンス
// メソッドとしてコンパイルされる（staticではない）。そのためJNI側の第2引数は
// JClassではなくJObject（シングルトンインスタンス自身への参照）を受け取る
// （MainActivity.ktのnativeOnResume/nativeRestorePickedと同じ形）。

// Kotlin側がUsageStatsManagerで新規パッケージを検知し、かつPackageManagerで
// アプリ名の解決に成功した時だけ呼ぶ（アプリに紐づかないものは呼び出し元で除外済み、
// UsageReporter.kt参照）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sleeptracker_app_UsageReporter_nativeRecordUsagePackageSeen<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    package: JString<'caller>,
    label: JString<'caller>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let pkg: String = package.try_to_string(env)?;
            let lbl: String = label.try_to_string(env)?;
            if let Err(e) = events::record_usage_package_seen(&pkg, &lbl) {
                eprintln!("[app] android usage: ERROR record_usage_package_seen: {}", e);
            }
            Ok(())
        })
        .resolve::<LogErrorAndDefault>();
}

// Kotlin側が使用区間の判定前に1回だけ呼び、OFFにされているパッケージ名の
// カンマ区切り文字列を受け取る（1件ずつJNI呼び出しするより効率的なため）。
// 戻り値の型はJObject（jstring/jobjectはJNIのC ABIレベルでは同じ表現のため、
// JObject::default()（null）をエラー時の既定値として使える）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sleeptracker_app_UsageReporter_nativeGetDeniedPackages<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
) -> JObject<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<JObject<'caller>> {
            let denied: Vec<String> = events::list_usage_packages()
                .into_iter()
                .filter(|e| !e.allowed)
                .map(|e| e.package)
                .collect();
            let s = env.new_string(denied.join(","))?;
            Ok(s.into())
        })
        .resolve::<LogErrorAndDefault>()
}

// Kotlin側が使用区間のスキャンを終えるたびに呼ぶ。「同期を停止」中でも動く
// bg::refresh_ui()をキックし、新規検知パッケージが設定画面に反映
// されるようにする（JNI呼び出し自体は不要なのでEnvUnowned/JObjectは未使用）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sleeptracker_app_UsageReporter_nativeReportUsageScanComplete<'caller>(
    _unowned_env: EnvUnowned<'caller>,
    _this: JObject<'caller>,
) {
    super::bg::refresh_ui();
}
