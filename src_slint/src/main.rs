//! main.rs — デスクトップ版エントリポイント
//!
//! 役割 : lib.rs の run() を呼ぶだけの薄いラッパー。
//!        共通ロジックは全て lib.rs / 各モジュールに実装されている
//!        （Android版は src/android.rs から同じ lib.rs::run() を呼ぶ）。

fn main() {
    sleep_tracker::run();
}
