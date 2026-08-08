//! GAG-010 / goal: per-task model & reasoning settings, ACP slash-command
//! discovery, clipboard blob imports, and auto-derived task titles.
//!
//! Public seam: DesktopBridge dispatcher backed by real SQLite,
//! TaskRuntime, AgentRuntime, and the process-based Fake ACP agent.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl, DesktopResult};
use grok_acp_gui_lib::domain::types::{utc_now, Project, ProjectId};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::{TaskRuntime, TaskRuntimeImpl};

fn fake_agent_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

async fn make_repo_with_project(project_id: &str) -> Arc<SqliteRepository> {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new(project_id),
        path: cwd.to_string_lossy().into_owned(),
        display_path: format!("{project_id}-fixture"),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    repo
}

async fn create_task(
    repo: &SqliteRepository,
    runtime: &dyn AgentRuntime,
    task_runtime: &dyn TaskRuntime,
    project_id: &str,
    prompt: &str,
    title: Option<&str>,
) -> String {
    let mut payload = serde_json::json!({
        "projectId": project_id,
        "prompt": prompt,
        "mode": "ask",
        "workspaceStrategy": "direct"
    });
    if let Some(title) = title {
        payload["title"] = serde_json::json!(title);
    }
    let result = execute_impl(
        repo,
        runtime,
        task_runtime,
        serde_json::json!({ "type": "task.create", "payload": payload }),
    )
    .await;
    match result {
        DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
        DesktopResult::Err { error } => panic!("create failed: {error:?}"),
    }
}

/// Drain the bridge event stream until `predicate` matches; returns the
/// first matching event.
async fn wait_for_event(
    mut events: tokio::sync::broadcast::Receiver<grok_acp_gui_lib::bridge::events::DesktopEvent>,
    predicate: impl Fn(&grok_acp_gui_lib::bridge::events::DesktopEvent) -> bool,
) -> grok_acp_gui_lib::bridge::events::DesktopEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        if timeout.is_zero() {
            panic!("timed out waiting for bridge event");
        }
        match tokio::time::timeout(timeout, events.recv()).await {
            Ok(Ok(event)) if predicate(&event) => return event,
            Ok(Err(_)) => panic!("event channel closed"),
            _ => continue,
        }
    }
}

#[tokio::test]
async fn session_configure_persists_and_next_prompt_carries_model_and_reasoning() {
    let repo = make_repo_with_project("project-chat-settings").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-chat-settings",
        "first message",
        Some("Chat settings"),
    )
    .await;
    // Let the first turn finish so the session is idle for the next send.
    wait_for_event(events.resubscribe(), |event| {
        event.event_type == "task.state"
            && event.payload.get("status").and_then(|v| v.as_str()) == Some("idle")
    })
    .await;

    // Configure a new model + reasoning for the task.
    let configured = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": task_id,
                "settings": { "model": "deepseek-v4-pro", "reasoning": "max" }
            }
        }),
    )
    .await;
    match configured {
        DesktopResult::Ok { data } => {
            assert_eq!(data["model"], "deepseek-v4-pro");
            assert_eq!(data["reasoning"], "max");
        }
        DesktopResult::Err { error } => panic!("configure failed: {error:?}"),
    }

    // The persisted task must carry the new selection.
    let task = repo.get_task(&task_id).expect("task query");
    assert_eq!(task.model.as_deref(), Some("deepseek-v4-pro"));
    assert_eq!(task.reasoning.as_deref(), Some("max"));

    // task.open must surface the selection so a reopened conversation
    // restores the controls.
    let reopened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "task.open", "payload": { "taskId": task_id } }),
    )
    .await;
    match reopened {
        DesktopResult::Ok { data } => {
            assert_eq!(data["model"], "deepseek-v4-pro");
            assert_eq!(data["reasoning"], "max");
        }
        DesktopResult::Err { error } => panic!("open failed: {error:?}"),
    }

    // The next turn must carry the new model/reasoning in the ACP
    // session/prompt request. The fake agent echoes what it received.
    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": task_id, "message": "second message" }
        }),
    )
    .await;
    assert!(matches!(sent, DesktopResult::Ok { .. }), "{sent:?}");

    let mut streamed = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline && !streamed.contains("MODEL=") {
        if let Ok(Ok(event)) = tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            if event.event_type == "message.delta" {
                streamed.push_str(
                    event
                        .payload
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                );
            }
        }
    }
    assert!(
        streamed.contains("MODEL=deepseek-v4-pro REASONING=max"),
        "the session/prompt params were not echoed back: {streamed:?}"
    );

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn session_configure_rejects_unknown_reasoning_and_invalid_model() {
    let repo = make_repo_with_project("project-config-reject").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-config-reject",
        "hello",
        Some("Reject"),
    )
    .await;

    let bad_reasoning = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": task_id,
                "settings": { "reasoning": "ultra" }
            }
        }),
    )
    .await;
    assert!(
        matches!(bad_reasoning, DesktopResult::Err { .. }),
        "unknown reasoning must be rejected"
    );

    let bad_model = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": task_id,
                "settings": { "model": "--always-approve" }
            }
        }),
    )
    .await;
    assert!(
        matches!(bad_model, DesktopResult::Err { .. }),
        "option-like model id must fail closed"
    );

    let task = repo.get_task(&task_id).expect("task query");
    assert_eq!(task.model, None);
    assert_eq!(task.reasoning, None);

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn available_commands_update_reaches_the_bridge_as_typed_event() {
    let repo = make_repo_with_project("project-commands").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::CommandsUpdate,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let events = task_runtime.event_subscriber();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-commands",
        "hello",
        Some("Commands"),
    )
    .await;

    let event = wait_for_event(events, |event| {
        event.event_type == "session.commands.updated"
    })
    .await;
    assert_eq!(
        event.task_id.as_ref().map(|id| id.0.as_str()),
        Some(task_id.as_str())
    );
    let commands = event
        .payload
        .get("commands")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["name"], "init");
    assert_eq!(commands[0]["description"], "Initialize a new project");
    assert_eq!(commands[0]["acceptsInput"], false);
    assert_eq!(commands[1]["name"], "plan");
    assert_eq!(commands[1]["acceptsInput"], true);

    // The typed event must survive persistence and reopen as history too.
    let reopened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "task.open", "payload": { "taskId": task_id } }),
    )
    .await;
    let data = match reopened {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("open failed: {error:?}"),
    };
    assert!(data["events"].as_array().unwrap().iter().any(|event| {
        event["type"] == "session.commands.updated"
            && event["payload"]["commands"][0]["name"] == "init"
    }));

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn session_configure_mode_persists_and_next_turn_set_mode_is_observable() {
    let repo = make_repo_with_project("project-mode-switch").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-mode-switch",
        "first message",
        Some("Mode switch"),
    )
    .await;
    // Let the first turn finish so the session is idle for the next send.
    wait_for_event(events.resubscribe(), |event| {
        event.event_type == "task.state"
            && event.payload.get("status").and_then(|v| v.as_str()) == Some("idle")
    })
    .await;

    // Switch the task to Plan mode.
    let configured = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": task_id,
                "settings": { "mode": "plan" }
            }
        }),
    )
    .await;
    match configured {
        DesktopResult::Ok { data } => {
            assert_eq!(data["mode"], "plan");
        }
        DesktopResult::Err { error } => panic!("configure failed: {error:?}"),
    }

    let task = repo.get_task(&task_id).expect("task query");
    assert_eq!(task.mode.as_deref(), Some("plan"));

    // task.open must surface the mode so a reopened conversation restores it.
    let reopened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "task.open", "payload": { "taskId": task_id } }),
    )
    .await;
    match reopened {
        DesktopResult::Ok { data } => {
            assert_eq!(data["mode"], "plan");
        }
        DesktopResult::Err { error } => panic!("open failed: {error:?}"),
    }

    // The next turn must send session/set_mode with modeId=plan before the
    // prompt; the fake agent echoes the active mode in the stream.
    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": task_id, "message": "second message" }
        }),
    )
    .await;
    assert!(matches!(sent, DesktopResult::Ok { .. }), "{sent:?}");

    let mut streamed = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline && !streamed.contains("MODE=plan") {
        if let Ok(Ok(event)) = tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            if event.event_type == "message.delta" {
                streamed.push_str(
                    event
                        .payload
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                );
            }
        }
    }
    assert!(
        streamed.contains("MODE=plan"),
        "the session/set_mode modeId was not echoed back: {streamed:?}"
    );

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn session_configure_rejects_invalid_or_oversized_mode_values() {
    let repo = make_repo_with_project("project-mode-reject").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-mode-reject",
        "hello",
        Some("Mode reject"),
    )
    .await;

    let configure = |settings: serde_json::Value| {
        execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({
                "type": "session.configure",
                "payload": { "taskId": task_id, "settings": settings }
            }),
        )
    };

    // Illegal characters are rejected.
    let bad = configure(serde_json::json!({ "mode": "a/b c" })).await;
    assert!(
        matches!(bad, DesktopResult::Err { .. }),
        "invalid mode accepted"
    );

    // Overlong mode ids are rejected.
    let long = "x".repeat(65);
    let oversized = configure(serde_json::json!({ "mode": long })).await;
    assert!(
        matches!(oversized, DesktopResult::Err { .. }),
        "oversized mode accepted"
    );

    // A mode value that is not a string is rejected.
    let non_string = configure(serde_json::json!({ "mode": 42 })).await;
    assert!(
        matches!(non_string, DesktopResult::Err { .. }),
        "non-string mode accepted"
    );

    // Nothing changed the persisted mode (create_task used the initial "ask").
    let task = repo.get_task(&task_id).expect("task query");
    assert_eq!(task.mode.as_deref(), Some("ask"));

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn unadvertised_mode_fails_the_turn_with_acp_request_failed() {
    let repo = make_repo_with_project("project-mode-unadvertised").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let events = task_runtime.event_subscriber();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-mode-unadvertised",
        "hello",
        Some("Mode unadvertised"),
    )
    .await;
    // Let the first turn finish so the session is idle for the next send.
    wait_for_event(events.resubscribe(), |event| {
        event.event_type == "task.state"
            && event.payload.get("status").and_then(|v| v.as_str()) == Some("idle")
    })
    .await;

    // The fake agent advertises default/plan/code/ask — "unknown-mode" is
    // syntactically valid but not advertised.
    let configured = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": task_id,
                "settings": { "mode": "unknown-mode" }
            }
        }),
    )
    .await;
    assert!(
        matches!(configured, DesktopResult::Ok { .. }),
        "configure must accept the value; the ACP layer validates advertisement"
    );

    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": task_id, "message": "this must fail" }
        }),
    )
    .await;
    match sent {
        DesktopResult::Err { error } => {
            assert_eq!(
                error.code, "ACP_REQUEST_FAILED",
                "unadvertised mode must fail closed: {error:?}"
            );
        }
        DesktopResult::Ok { .. } => panic!("unadvertised mode must not send a turn"),
    }

    // The task remains usable: switching back to an advertised mode works.
    let revert = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": task_id,
                "settings": { "mode": "ask" }
            }
        }),
    )
    .await;
    assert!(matches!(revert, DesktopResult::Ok { .. }), "{revert:?}");
    let retried = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": task_id, "message": "retry after revert" }
        }),
    )
    .await;
    assert!(
        matches!(retried, DesktopResult::Ok { .. }),
        "retry after reverting to an advertised mode must succeed: {retried:?}"
    );

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn empty_task_title_is_derived_from_the_first_sentence() {
    let repo = make_repo_with_project("project-derived-title").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    let task_id = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-derived-title",
        "实现登录页面。包含邮箱和密码校验。",
        None,
    )
    .await;
    let task = repo.get_task(&task_id).expect("task query");
    assert_eq!(task.title, "实现登录页面");
    assert_ne!(task.title, "未命名任务");

    // Multi-line prompts use the first non-empty line.
    let second = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-derived-title",
        "\n第二行开始\n还是第二行",
        None,
    )
    .await;
    let second_task = repo.get_task(&second).expect("task query");
    assert_eq!(second_task.title, "第二行开始");

    // Over-long sentences are truncated with an ellipsis.
    let long: String = "优化"
        .chars()
        .chain(std::iter::repeat('长'))
        .take(80)
        .collect();
    let third = create_task(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        "project-derived-title",
        &long,
        None,
    )
    .await;
    let third_task = repo.get_task(&third).expect("task query");
    assert!(third_task.title.ends_with('…'));
    assert!(third_task.title.chars().count() <= 31);

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn reasoning_max_is_accepted_by_task_create() {
    let repo = make_repo_with_project("project-reasoning-max").await;
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    let result = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-reasoning-max",
                "title": "Max reasoning",
                "prompt": "hello",
                "mode": "ask",
                "reasoning": "max",
                "workspaceStrategy": "direct"
            }
        }),
    )
    .await;
    match result {
        DesktopResult::Ok { data } => {
            let task_id = data["taskId"].as_str().unwrap();
            let task = repo.get_task(task_id).expect("task query");
            assert_eq!(task.reasoning.as_deref(), Some("max"));
        }
        DesktopResult::Err { error } => panic!("max reasoning rejected: {error:?}"),
    }

    runtime.shutdown_all("test complete").await;
}
