//! FreeLip Tauri application scaffold.

#[cfg(windows)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to run FreeLip Tauri shell");
}

#[cfg(not(windows))]
pub fn run() {
    eprintln!("FreeLip Tauri shell is Windows-only; contract tests remain cross-platform.");
}
