//! main.rs — デスクトップ版エントリポイント
//!
//! 役割 : lib.rs の run() を呼ぶだけの薄いラッパー。
//!        共通ロジックは全て lib.rs / 各モジュールに実装されている
//!        （Android版は src/android.rs から同じ lib.rs::run() を呼ぶ）。
//!
//! リリースビルドでは起動時にコンソールウィンドウが表示されないよう
//! windows_subsystem を指定する（デバッグビルドではeprintln!のログを
//! 確認できるようコンソールを残す）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Windows環境によっては既定のOpenGL/FemtoVGがウィンドウだけ作成して
    // 内容を描画できないことがある。タスクバー起動は環境変数を引き継がないため、
    // 未指定時は安定して描画できるwinit software rendererを選ぶ。
    #[cfg(windows)]
    if std::env::var_os("SLINT_BACKEND").is_none() {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }
    sleep_tracker::run();
}
