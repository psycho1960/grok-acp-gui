mod app;
pub mod bridge;
pub mod domain;

use bridge::dispatch::{self as br_dispatch, DesktopResult};

#[tauri::command]
fn bootstrap() -> br_dispatch::BootstrapStatus {
    br_dispatch::bootstrap_impl()
}

#[tauri::command]
fn execute(command: serde_json::Value) -> DesktopResult {
    br_dispatch::execute_impl(command)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(app::configure)
        .invoke_handler(tauri::generate_handler![bootstrap, execute])
        .run(tauri::generate_context!())
        .expect("error while running Grok ACP GUI");
}
