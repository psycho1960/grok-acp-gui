use tauri::Manager;

pub const MAIN_WINDOW_LABEL: &str = "main";

/// Configure the application-level composition root after Tauri creates the
/// main window. Runtime modules are intentionally not started by GAG-001.
pub fn configure(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    debug_assert!(app.get_webview_window(MAIN_WINDOW_LABEL).is_some());
    Ok(())
}
