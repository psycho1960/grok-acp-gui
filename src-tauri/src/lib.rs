mod app;
pub mod bridge;
pub mod domain;
pub mod modules {
    pub mod persistence;
}
pub mod adapters {
    pub mod sqlite;
}

use crate::modules::persistence::Repository;
use bridge::dispatch::{self as br_dispatch, DesktopResult};
use std::sync::Arc;
use tauri::Manager;

/// Shared application state accessible by Tauri commands.
pub struct AppState {
    pub repo: Arc<dyn crate::modules::persistence::Repository>,
}

#[tauri::command]
fn bootstrap(state: tauri::State<'_, AppState>) -> br_dispatch::BootstrapSnapshot {
    br_dispatch::bootstrap_impl(state.repo.as_ref())
}

#[tauri::command]
fn execute(state: tauri::State<'_, AppState>, command: serde_json::Value) -> DesktopResult {
    br_dispatch::execute_impl(state.repo.as_ref(), command)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            app::configure(app)?;
            // Initialize the SQLite repository for the app.
            // Database path is derived from the app data directory.
            let app_handle = app.handle();
            let data_dir = app_handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("grok_acp_gui.db");

            let repo = adapters::sqlite::SqliteRepository::open(&db_path)
                .expect("Failed to initialize database");

            // Perform startup recovery.
            let interrupted = repo
                .recover_interrupted_tasks("Application exited unexpectedly")
                .unwrap_or(0);
            if interrupted > 0 {
                eprintln!("Recovered {} interrupted task(s)", interrupted);
            }

            app.manage(AppState {
                repo: Arc::new(repo),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![bootstrap, execute])
        .run(tauri::generate_context!())
        .expect("error while running Grok ACP GUI");
}
