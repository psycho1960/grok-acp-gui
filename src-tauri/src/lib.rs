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

pub struct AppState {
    pub repo: Arc<dyn Repository>,
    pub db_init_error: Option<String>,
}

#[tauri::command]
fn bootstrap(state: tauri::State<'_, AppState>) -> br_dispatch::BootstrapSnapshot {
    br_dispatch::bootstrap_impl(state.repo.as_ref(), state.db_init_error.as_deref())
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
            let app_handle = app.handle();
            let data_dir = app_handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            if let Err(e) = std::fs::create_dir_all(&data_dir) {
                eprintln!("WARNING: could not create data directory {:?}: {}", data_dir, e);
            }
            let db_path = data_dir.join("grok_acp_gui.db");
            let mut db_init_error: Option<String> = None;
            let repo: Arc<dyn Repository> = match adapters::sqlite::SqliteRepository::open(&db_path) {
                Ok(r) => {
                    match r.recover_interrupted_tasks("Application exited unexpectedly") {
                        Ok(interrupted) if interrupted > 0 => {
                            eprintln!("Recovered {} interrupted task(s)", interrupted);
                        }
                        Err(e) => {
                            eprintln!("WARNING: startup recovery failed ({}): {}", e.code, e.message);
                        }
                        _ => {}
                    }
                    Arc::new(r)
                }
                Err(e) => {
                    eprintln!(
                        "FATAL: database initialisation failed ({}): {}. Falling back to in-memory store.",
                        e.code, e.message
                    );
                    db_init_error = Some(format!(
                        "Database unavailable ({}). Restart the application. If the problem persists, delete {} and restart.",
                        e.message, db_path.display()
                    ));
                    Arc::new(
                        adapters::sqlite::SqliteRepository::open_in_memory()
                            .expect("in-memory fallback must succeed"),
                    )
                }
            };
            app.manage(AppState { repo, db_init_error });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![bootstrap, execute])
        .run(tauri::generate_context!())
        .expect("error while running Grok ACP GUI");
}
