fn main() {
    slint_build::compile("ui/main.slint").unwrap();

    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("アイコンの埋め込みに失敗しました");
    }
}
