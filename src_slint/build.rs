fn main() {
    slint_build::compile("ui/main.slint").unwrap();

    // build.rs内の#[cfg(windows)]はビルドを実行しているホストのOSを見てしまい、
    // Windows機からAndroid向けにクロスコンパイルする際もtrueになってしまう
    // （#[cfg]はホスト判定、TARGETを見るにはCARGO_CFG_TARGET_OSを使う必要がある）。
    // これが原因でAndroidビルドがrc.exe不在で失敗していたため、TARGET側で判定する。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("アイコンの埋め込みに失敗しました");
    }
}
