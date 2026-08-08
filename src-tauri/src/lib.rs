mod app;
pub mod bridge;
pub mod domain;
pub mod modules {
    pub mod agent_runtime;
    pub mod artifacts;
    pub mod persistence;
    pub mod task_runtime;
}
pub mod adapters {
    pub mod filesystem;
    pub mod grok_acp;
    pub mod sqlite;
}

use crate::adapters::grok_acp::GrokAcpAdapter;
use crate::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl, RuntimeConfig};
use crate::modules::artifacts::{ArtifactService, ManagedArtifactService};
use crate::modules::persistence::Repository;
use crate::modules::task_runtime::{TaskRuntime, TaskRuntimeImpl};
use bridge::dispatch::{self as br_dispatch, DesktopResult};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

pub struct AppState {
    pub repo: Arc<dyn Repository>,
    pub runtime: Arc<dyn AgentRuntime>,
    pub task_runtime: Arc<dyn TaskRuntime>,
    pub db_init_error: Option<String>,
}

fn application_data_dir(app_handle: &AppHandle) -> PathBuf {
    #[cfg(feature = "e2e-isolated-data")]
    if let Some(value) = std::env::var_os("GROK_ACP_GUI_E2E_DATA_DIR") {
        let candidate = PathBuf::from(value);
        if candidate.is_absolute() {
            return candidate;
        }
        eprintln!("WARNING: ignored relative GROK_ACP_GUI_E2E_DATA_DIR; using managed app data");
    }

    app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[tauri::command]
async fn bootstrap(
    state: tauri::State<'_, AppState>,
) -> Result<br_dispatch::BootstrapSnapshot, String> {
    let mut snapshot =
        br_dispatch::bootstrap_impl(state.repo.as_ref(), state.db_init_error.as_deref());
    if snapshot.ready {
        let probe = state.runtime.probe(&RuntimeConfig::default()).await;
        snapshot.runtime = br_dispatch::RuntimeBootstrapStatus {
            status: if probe.available {
                "ready"
            } else {
                "unavailable"
            }
            .into(),
            probe_error: if probe.available {
                None
            } else {
                probe.message.or(Some(probe.status))
            },
            version: probe.version,
            authenticated: probe.authenticated,
        };
    }
    Ok(snapshot)
}

#[tauri::command]
fn execute(state: tauri::State<'_, AppState>, command: serde_json::Value) -> DesktopResult {
    // Clone Arcs out of state to get 'static references for the async block.
    let repo = state.inner().repo.clone();
    let runtime = state.inner().runtime.clone();
    let task_runtime = state.inner().task_runtime.clone();
    tauri::async_runtime::block_on(async move {
        br_dispatch::execute_impl(&*repo, &*runtime, &*task_runtime, command).await
    })
}

/// The renderer can only request an already-indexed task artifact through this
/// protocol.  It never receives a filesystem path or a `file:` URL, and the
/// service repeats owner/root/hash validation before any bytes are returned.
fn serve_artifact_protocol(
    app_handle: &AppHandle,
    request_path: &str,
) -> tauri::http::Response<Vec<u8>> {
    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .header("Cache-Control", "no-store")
            .body(Vec::new())
            .expect("static artifact response")
    };
    let Some((task_id, artifact_id)) = protocol_artifact_ids(request_path) else {
        return not_found();
    };
    let state = app_handle.state::<AppState>();
    let images = match ManagedArtifactService::new().resolve_images(
        state.repo.as_ref(),
        &bridge::types::TaskId::new(task_id.to_owned()),
        &[artifact_id.to_owned()],
    ) {
        Ok(images) if images.len() == 1 => images,
        _ => return not_found(),
    };
    let image = &images[0];
    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", image.descriptor.mime_type.as_str())
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .body(image.bytes.clone())
        .expect("static artifact response")
}

fn protocol_artifact_ids(path: &str) -> Option<(&str, &str)> {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() != 2 {
        return None;
    }
    let safe_id = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    };
    (safe_id(segments[0]) && safe_id(segments[1])).then_some((segments[0], segments[1]))
}

pub fn run() {
    let app = tauri::Builder::default()
        .register_uri_scheme_protocol("grok-artifact", |context, request| {
            serve_artifact_protocol(context.app_handle(), request.uri().path())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            app::configure(app)?;
            let app_handle = app.handle();

            // --- Database / persistence ---
            let data_dir = application_data_dir(app_handle);
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
            let runtime_impl = AgentRuntimeImpl::new(adapter);

            // --- Task runtime (GAG-006): task/session isolation, ordering,
            // persistence, and task-scoped event publication. ---
            let task_runtime_impl = Arc::new(TaskRuntimeImpl::new(
                repo.clone(),
                runtime_impl.clone(),
            ));
            task_runtime_impl.spawn_agent_event_forwarder();
            let bridge_events = task_runtime_impl.event_subscriber();
            let runtime: Arc<dyn AgentRuntime> = runtime_impl;
            let task_runtime: Arc<dyn TaskRuntime> = task_runtime_impl;

            // Events reach the Renderer only after TaskRuntime persisted and
            // attached the owning taskId.
            spawn_event_forwarder(app_handle.clone(), bridge_events);

            app.manage(AppState {
                repo,
                runtime,
                task_runtime,
                db_init_error,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![bootstrap, execute])
        .build(tauri::generate_context!())
        .expect("error while running Grok ACP GUI");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let runtime = app_handle
                .try_state::<AppState>()
                .map(|state| state.runtime.clone());
            if let Some(runtime) = runtime {
                tauri::async_runtime::block_on(runtime.shutdown_all("application exit"));
            }
        }
    });
}

/// Spawn a background task that reads events from the Agent Runtime
/// and forwards them to the Renderer via the `bridge:event` Tauri channel.
///
/// This is the ONLY path for runtime events to reach the UI.  The
/// Renderer never touches raw stdout, JSON-RPC frames, or process handles.
fn spawn_event_forwarder(
    app_handle: AppHandle,
    mut events: tokio::sync::broadcast::Receiver<bridge::events::DesktopEvent>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app_handle.emit(br_dispatch::EVENT_CHANNEL, &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    eprintln!("bridge event receiver lagged; skipped {skipped} event(s)");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::protocol_artifact_ids;

    #[test]
    fn artifact_protocol_accepts_only_opaque_ids() {
        assert_eq!(
            protocol_artifact_ids("/task-1/artifact_2"),
            Some(("task-1", "artifact_2"))
        );
        assert!(protocol_artifact_ids("/task-1/../secret").is_none());
        assert!(protocol_artifact_ids("/task-1/%2e%2e").is_none());
        assert!(protocol_artifact_ids("/task-1/artifact-2/extra").is_none());
    }
}
