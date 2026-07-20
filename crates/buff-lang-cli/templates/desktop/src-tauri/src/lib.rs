//! {name} — Tauri 2.0 desktop app.
//!
//! Loads the Buff-Wasm-Dioxus frontend from `../static/` into a
//! native webview window. The Wasm bundle is built by the Buff
//! pipeline (`buff ui build --desktop`).
//!
//! # IPC bridge
//!
//! Tauri commands exposed to the frontend via `#[tauri::command]`.
//! Add new commands here and register them in `run()`.

use tauri::Manager;

/// Example IPC command: greet the user from the Rust backend.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! From {name} desktop app.", name)
}

/// Run the Tauri application.
///
/// Called from `main.rs`. Registers all commands and plugins.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
