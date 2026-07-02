//! android.rs — Android版エントリポイント
//!
//! 役割 : cargo-apk が呼び出す android_main。Slintのandroid-activityバックエンドを
//!        初期化し、アプリのデータ保存先（内部ストレージ）をlib.rsに伝えたうえで
//!        lib.rs::run() を呼ぶ（デスクトップと共通のロジックをそのまま使う）。

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let data_dir = app
        .internal_data_path()
        .expect("internal_data_path が取得できませんでした");
    crate::init_android_app_dir(data_dir);

    slint::android::init(app).expect("Slint Androidバックエンドの初期化に失敗しました");
    crate::run();
}
