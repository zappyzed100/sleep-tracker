//! paths.rs — データ/設定ファイルの保存先パス解決・共有HTTPクライアント
//!
//! 役割 : デスクトップ（exeの場所からui/main.slintをマーカーに自己完結ディレクトリを
//!        探す）・Android（setup()で渡されるアプリ内部/外部ストレージパス）の両方に
//!        対応したdata_dir/config_path/backups_base_dirと、使い回すHTTPクライアント
//!        （接続プーリング）を提供する。lib.rsから`pub use paths::*`で再公開され、
//!        crate全体から`crate::data_dir()`等でそのまま呼べる。
//!
//! 公開 : `THRESHOLD_SECS`, `MIN_SCREEN_ON_SECS`, `data_dir`, `config_path`, `backups_base_dir`,
//!        `http_client`, `init_android_app_dir`, `init_android_external_dir`,
//!        `android_external_dir`（Android専用）

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::OnceLock;

// Shared threshold: updated instantly by save_config, read by monitor thread.
pub static THRESHOLD_SECS: AtomicU64 = AtomicU64::new(3600);

// SCREEN_ON区間のうち、この秒数未満は「一瞬触れただけ」として睡眠判定から無視する。
// save_configから更新され、core::events::parsingが都度ファイルを読まずに使える
// （THRESHOLD_SECSと同じパターン）。
pub static MIN_SCREEN_ON_SECS: AtomicU64 = AtomicU64::new(300);

static HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

// ── Path utilities ────────────────────────────────────────────────────────────
//
// デスクトップ: exe の場所から上に辿って ui/main.slint をマーカーにsrc_slint/
// 自身を探す。データ・設定ファイルはsrc_slint/配下に自己完結させる
// （以前はTauri版のsrc_tauri/data/を間借りしていたが、src_tauri/を削除すると
// データを見失う脆い作りだったため、src_slint単体で完結するよう変更した）。
// Android: setup()でAndroidのアプリ内部ストレージパスを渡してもらう（Tauri版のAPP_DIR相当）。

#[cfg(target_os = "android")]
static ANDROID_APP_DIR: OnceLock<PathBuf> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn init_android_app_dir(path: PathBuf) {
    let _ = ANDROID_APP_DIR.set(path);
}

// アプリ専用の外部ストレージ領域（/storage/emulated/0/Android/data/<package>/files/）。
// スコープドストレージ配下でも特別な権限なしにファイルマネージャーから参照できるため、
// CSVエクスポート・バックアップ・リストア（rfdが使えないAndroidの代替）に使う。
#[cfg(target_os = "android")]
static ANDROID_EXTERNAL_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn init_android_external_dir(path: Option<PathBuf>) {
    let _ = ANDROID_EXTERNAL_DIR.set(path);
}

#[cfg(target_os = "android")]
pub fn android_external_dir() -> Option<PathBuf> {
    ANDROID_EXTERNAL_DIR.get().cloned().flatten()
}

#[cfg(not(target_os = "android"))]
fn app_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        let mut dir = exe.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        for _ in 0..8 {
            if dir.join("ui").join("main.slint").exists() { return dir; }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
        std::env::current_dir().unwrap_or_default()
    })
}

#[cfg(not(target_os = "android"))]
pub fn data_dir() -> PathBuf {
    static DATA: OnceLock<PathBuf> = OnceLock::new();
    DATA.get_or_init(|| {
        let dir = app_root().join("data");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }).clone()
}

#[cfg(target_os = "android")]
pub fn data_dir() -> PathBuf {
    ANDROID_APP_DIR.get().expect("init_android_app_dir が呼ばれていません").clone()
}

#[cfg(not(target_os = "android"))]
pub fn config_path() -> PathBuf {
    app_root().join("config.json")
}

#[cfg(target_os = "android")]
pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

// 自動バックアップ・手動バックアップ削除の書き込み先ベースディレクトリ。
// PCはdata_dirと同じ場所でよいが、Androidのdata_dir()はアプリ内部保存領域
// （スコープドストレージの外からは一切アクセスできない）なので、CSVエクスポート・
// 手動バックアップと同じ「ファイルマネージャーから参照できる外部ストレージ領域」
// （android_external_dir）に書き出す。
#[cfg(not(target_os = "android"))]
pub fn backups_base_dir() -> PathBuf {
    data_dir()
}

#[cfg(target_os = "android")]
pub fn backups_base_dir() -> PathBuf {
    android_external_dir().unwrap_or_else(data_dir)
}

pub fn http_client() -> Result<&'static reqwest::blocking::Client, String> {
    if HTTP_CLIENT.get().is_none() {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let _ = HTTP_CLIENT.set(client);
    }
    Ok(HTTP_CLIENT.get().unwrap())
}
