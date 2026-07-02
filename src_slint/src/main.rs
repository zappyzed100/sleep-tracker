//! main.rs — sleep_tracker (Rust + Slint) エントリポイント
//!
//! 役割 : Slintウィンドウを起動する。動作確認用の最小構成。
//!        ビジネスロジック（config/events/cloud/prediction）は今後 src/ 配下に移植する。

slint::include_modules!();

fn main() {
    let window = MainWindow::new().expect("ウィンドウの作成に失敗しました");
    window.set_greeting("Rust + Slint 起動成功".into());
    window.run().expect("ウィンドウの実行に失敗しました");
}
