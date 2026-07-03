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
    sleep_tracker::run();
}
