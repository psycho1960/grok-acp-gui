use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use grok_acp_gui_lib::adapters::grok_acp::fake::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl_with_vision, DesktopResult};
use grok_acp_gui_lib::bridge::types::TaskId;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, Task, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl};
use grok_acp_gui_lib::modules::artifacts::ManagedArtifactService;
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::TaskRuntimeImpl;

fn fake_agent_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

#[tokio::test]
async fn image_is_analyzed_by_luna_then_only_text_reaches_main_session() {
    let temp = std::env::temp_dir().join(format!("gag010-handoff-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp).unwrap();
    let image_path = temp.join("screen.png");
    let image = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .unwrap();
    std::fs::write(&image_path, image).unwrap();

    let repo = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let project = Project {
        id: ProjectId::new("project-luna-handoff"),
        path: temp.to_string_lossy().into_owned(),
        display_path: "fixture".into(),
        repo_root: None,
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    };
    repo.create_project(&project).unwrap();
    let task = Task {
        id: TaskId::new("task-luna-handoff"),
        project_id: project.id,
        title: "luna handoff".into(),
        status: TaskStatus::Idle,
        workspace_kind: WorkspaceKind::Direct,
        mode: None,
        model: Some("deepseek-v4-pro".into()),
        reasoning: Some("high".into()),
        created_at: utc_now(),
        updated_at: utc_now(),
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    };
    repo.create_task(&task).unwrap();

    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let vision_runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();
    let artifacts = ManagedArtifactService::new();

    let imported = execute_impl_with_vision(
        repo.as_ref(),
        runtime.as_ref(),
        vision_runtime.as_ref(),
        task_runtime.as_ref(),
        &artifacts,
        serde_json::json!({
            "type": "artifact.import",
            "payload": { "taskId": task.id, "paths": [image_path] }
        }),
    )
    .await;
    let artifact_id = match imported {
        DesktopResult::Ok { data } => data["artifacts"][0]["artifactId"]
            .as_str()
            .unwrap()
            .to_string(),
        DesktopResult::Err { error } => panic!("artifact import failed: {}", error.message),
    };

    let sent = execute_impl_with_vision(
        repo.as_ref(),
        runtime.as_ref(),
        vision_runtime.as_ref(),
        task_runtime.as_ref(),
        &artifacts,
        serde_json::json!({
            "type": "turn.send",
            "payload": {
                "taskId": task.id,
                "message": "What is shown?",
                "attachments": [artifact_id]
            }
        }),
    )
    .await;
    assert!(matches!(sent, DesktopResult::Ok { .. }));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut main_text = String::new();
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) if event.task_id.as_ref() == Some(&task.id) => {
                if event.event_type == "message.delta" && event.payload["role"] == "assistant" {
                    main_text.push_str(event.payload["text"].as_str().unwrap_or_default());
                }
                if event.event_type == "task.state" && event.payload["status"] == "idle" {
                    break;
                }
            }
            Ok(Ok(_)) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }

    runtime.shutdown_all("handoff test complete").await;
    vision_runtime.shutdown_all("handoff test complete").await;
    assert_eq!(main_text, "MAIN_TEXT_ONLY_OK");
    std::fs::remove_dir_all(temp).unwrap();
}
