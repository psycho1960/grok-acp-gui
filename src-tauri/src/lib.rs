mod app;
pub mod bridge;
pub mod domain;
pub mod modules {
    pub mod agent_runtime;
    pub mod persistence;
}
pub mod adapters {
    pub mod grok_acp;
    pub mod sqlite;
}

use crate::adapters::grok_acp::GrokAcpAdapter;
use crate::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl, RuntimeConfig};
use crate::modules::persistence::Repository;
use bridge::dispatch::{self as br_dispatch, DesktopResult};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

pub struct AppState {
    pub repo: Arc<dyn Repository>,
    pub runtime: Arc<dyn AgentRuntime>,
    pub db_init_error: Option<String>,
}

#[tauri::command]
fn bootstrap(state: tauri::State<'_, AppState>) -> br_dispatch::BootstrapSnapshot {
    br_dispatch::bootstrap_impl(state.repo.as_ref(), state.db_init_error.as_deref())
}

#[tauri::command]
fn execute(state: tauri::State<'_, AppState>, command: serde_json::Value) -> DesktopResult {
    // Clone Arcs out of state to get 'static references for the async block.
    let repo = state.inner().repo.clone();
    let runtime = state.inner().runtime.clone();
    tauri::async_runtime::block_on(async move {
        br_dispatch::execute_impl(&*repo, &*runtime, command).await
    })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            app::configure(app)?;
            let app_handle = app.handle();

            // --- Database / persistence ---
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
                            Arc::new(r)
                        }
                        Ok(_) => Arc::new(r),
                        Err(e) => {
                            eprintln!(
                                "FATAL: startup recovery failed ({}): {}. Database cannot be trusted; falling back to in-memory store.",
                                e.code, e.message
                            );
                            db_init_error = Some(format!(
                                "Startup recovery failed ({}). {}. Restart the application. If the problem persists, delete {} and restart.",
                                e.code, e.message, db_path.display()
                            ));
                            Arc::new(
                                adapters::sqlite::SqliteRepository::open_in_memory()
                                    .expect("in-memory fallback must succeed"),
                            )
                        }
                    }
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

            // --- Agent runtime (GAG-005) ---
            let config = RuntimeConfig::default();
            let adapter = GrokAcpAdapter::new(config);
            let runtime: Arc<dyn AgentRuntime> = AgentRuntimeImpl::new(adapter);

            // --- Event forwarder: runtime events → bridge:event channel ---
            spawn_event_forwarder(app_handle.clone(), runtime.clone());

            app.manage(AppState {
                repo,
                runtime,
                db_init_error,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![bootstrap, execute])
        .run(tauri::generate_context!())
        .expect("error while running Grok ACP GUI");
}

/// Spawn a background task that reads events from the Agent Runtime
/// and forwards them to the Renderer via the `bridge:event` Tauri channel.
///
/// This is the ONLY path for runtime events to reach the UI.  The
/// Renderer never touches raw stdout, JSON-RPC frames, or process handles.
fn spawn_event_forwarder(app_handle: AppHandle, runtime: Arc<dyn AgentRuntime>) {
    tauri::async_runtime::spawn(async move {
        let mut rx = runtime.subscribe();
        while let Some(event) = rx.recv().await {
            // Map the AgentEvent to a DesktopEvent and emit it.
            if let Some(desktop_event) = br_dispatch::map_agent_event(event) {
                let _ = app_handle.emit(br_dispatch::EVENT_CHANNEL, &desktop_event);
            }
        }
    });
}
