use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapStatus {
    product_name: &'static str,
    version: &'static str,
    platform: &'static str,
    ready: bool,
}

/// Minimal renderer bootstrap seam. Runtime probing and ACP session setup
/// belong to later tasks and are intentionally not started here.
#[tauri::command]
fn bootstrap() -> BootstrapStatus {
    BootstrapStatus {
        product_name: "Grok ACP GUI",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        ready: true,
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![bootstrap])
        .run(tauri::generate_context!())
        .expect("error while running Grok ACP GUI");
}
