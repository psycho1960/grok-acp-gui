//! First-use runtime readiness assessment.
//!
//! This module owns the ordered startup state machine so the bridge remains a
//! thin deserialize/validate/map layer.  It deliberately returns only safe,
//! actionable metadata; environment values and raw child-process output never
//! cross this interface.

use super::{
    requests::PromptRequest, AgentEvent, AgentRuntime, ClientRequest, RuntimeConfig,
    RuntimeLoginResult, TimestampedEvent, WorkspaceContext,
};
use crate::bridge::types::SessionId;
use crate::domain;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupCheck {
    id: &'static str,
    label: &'static str,
    status: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionableRuntimeError {
    code: String,
    message: String,
    action: String,
    diagnostic: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReadinessSnapshot {
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    min_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    authenticated: Option<bool>,
    ready: bool,
    checks: Vec<StartupCheck>,
    login: RuntimeLoginResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    actionable_error: Option<ActionableRuntimeError>,
}

/// Run the ordered first-use readiness sequence. Authentication is verified
/// by a structured ACP Turn, never by parsing TUI or human-readable CLI text.
pub async fn assess(
    runtime: &dyn AgentRuntime,
    mut config: RuntimeConfig,
    selected_model: Option<String>,
    database_available: bool,
) -> RuntimeReadinessSnapshot {
    config.model = selected_model;
    let min_version = config.min_version.clone();
    let mut checks = Vec::with_capacity(7);

    checks.push(check_git().await);
    let probe = runtime.probe(&config).await;
    let installed = !matches!(probe.status.as_str(), "not_found");
    checks.push(if installed {
        StartupCheck {
            id: "grok",
            label: "Grok",
            status: "success",
            detail: "已找到 Grok CLI。".into(),
            code: None,
            action: None,
        }
    } else {
        StartupCheck {
            id: "grok",
            label: "Grok",
            status: "error",
            detail: "未找到 Grok CLI。".into(),
            code: Some(domain::error::codes::RUNTIME_NOT_FOUND.into()),
            action: Some("请按官方说明安装 Grok Build，然后重新检测。".into()),
        }
    });
    checks.push(match probe.status.as_str() {
        "version_too_low" => StartupCheck {
            id: "version",
            label: "版本",
            status: "error",
            detail: format!(
                "当前版本 {}，最低要求 {}。",
                probe.version.as_deref().unwrap_or("未知"),
                min_version
            ),
            code: Some(domain::error::codes::RUNTIME_PROBE_FAILED.into()),
            action: Some("运行 `grok update` 升级后重新检测。".into()),
        },
        "ready" => StartupCheck {
            id: "version",
            label: "版本",
            status: "success",
            detail: format!(
                "Grok {} 满足最低版本 {}。",
                probe.version.as_deref().unwrap_or("未知"),
                min_version
            ),
            code: None,
            action: None,
        },
        _ => StartupCheck {
            id: "version",
            label: "版本",
            status: if installed { "error" } else { "warning" },
            detail: "尚未取得可验证的 Grok 版本。".into(),
            code: installed.then(|| domain::error::codes::RUNTIME_PROBE_FAILED.into()),
            action: installed.then(|| "检查 Grok 安装后重新检测。".into()),
        },
    });

    let db_check = if database_available {
        StartupCheck {
            id: "database",
            label: "数据库",
            status: "success",
            detail: "应用数据库可用。".into(),
            code: None,
            action: None,
        }
    } else {
        StartupCheck {
            id: "database",
            label: "数据库",
            status: "error",
            detail: "应用数据库不可用。".into(),
            code: Some(domain::error::codes::DB_QUERY_FAILED.into()),
            action: Some("重启应用；如仍失败，请复制脱敏诊断并检查数据目录。".into()),
        }
    };
    let (workspace_check, workspace) = check_working_directory();

    let mut authenticated = None;
    let auth_check;
    let acp_check;
    if probe.status == "ready" && workspace.is_some() {
        let session_id = SessionId::new(format!("runtime-check-{}", uuid::Uuid::new_v4()));
        let mut runtime_events = runtime.subscribe();
        match runtime
            .start(
                session_id.clone(),
                WorkspaceContext {
                    cwd: workspace.clone().expect("checked workspace"),
                },
                &config,
            )
            .await
        {
            Ok(_) => {
                acp_check = StartupCheck {
                    id: "acp",
                    label: "ACP 握手",
                    status: "success",
                    detail: "ACP 握手与会话初始化成功。".into(),
                    code: None,
                    action: None,
                };
                auth_check = match verify_authentication_with_minimal_turn(
                    runtime,
                    &session_id,
                    &mut runtime_events,
                )
                .await
                {
                    AuthenticationVerification::Authenticated => {
                        authenticated = Some(true);
                        StartupCheck {
                            id: "authentication",
                            label: "认证",
                            status: "success",
                            detail: "Grok 已认证，并已通过最小模型请求验证。".into(),
                            code: None,
                            action: None,
                        }
                    }
                    AuthenticationVerification::AuthenticationRequired => {
                        authenticated = Some(false);
                        StartupCheck {
                            id: "authentication",
                            label: "认证",
                            status: "error",
                            detail: "Grok 尚未登录、认证已失效，或服务端拒绝了当前凭据。".into(),
                            code: Some(domain::error::codes::RUNTIME_LOGIN_FAILED.into()),
                            action: Some("点击“登录 Grok”完成官方登录流程。".into()),
                        }
                    }
                    AuthenticationVerification::Failed => StartupCheck {
                        id: "authentication",
                        label: "认证",
                        status: "error",
                        detail: "最小模型请求未成功，无法确认认证状态。".into(),
                        code: Some("RUNTIME_AUTH_PROBE_FAILED".into()),
                        action: Some("重新检测；如仍失败，请复制脱敏诊断。".into()),
                    },
                };
                runtime
                    .shutdown(session_id, "runtime readiness check complete")
                    .await;
            }
            Err(error) if error.code == domain::error::codes::RUNTIME_MODEL_ENV_MISSING => {
                auth_check = StartupCheck {
                    id: "authentication",
                    label: "认证",
                    status: "error",
                    detail: error.message,
                    code: Some(error.code),
                    action: Some("设置所示环境变量后完全退出并重启应用。".into()),
                };
                acp_check = skipped_acp_check();
                runtime
                    .shutdown(session_id, "runtime readiness check failed")
                    .await;
            }
            Err(error) if is_authentication_failure(&error.message) => {
                authenticated = Some(false);
                auth_check = StartupCheck {
                    id: "authentication",
                    label: "认证",
                    status: "error",
                    detail: "Grok 尚未登录或认证已失效。".into(),
                    code: Some(domain::error::codes::RUNTIME_LOGIN_FAILED.into()),
                    action: Some("点击“登录 Grok”完成官方登录流程。".into()),
                };
                acp_check = skipped_acp_check();
                runtime
                    .shutdown(session_id, "runtime authentication check failed")
                    .await;
            }
            Err(_) => {
                auth_check = StartupCheck {
                    id: "authentication",
                    label: "认证",
                    status: "warning",
                    detail: "ACP 握手失败，无法确认认证状态。".into(),
                    code: None,
                    action: Some("先重试；如仍失败，请复制脱敏诊断。".into()),
                };
                acp_check = StartupCheck {
                    id: "acp",
                    label: "ACP 握手",
                    status: "error",
                    detail: "ACP 握手或会话初始化失败。".into(),
                    code: Some(domain::error::codes::ACP_HANDSHAKE_FAILED.into()),
                    action: Some("重新检测；如仍失败，请更新 Grok 并复制脱敏诊断。".into()),
                };
                runtime
                    .shutdown(session_id, "runtime handshake check failed")
                    .await;
            }
        }
    } else {
        auth_check = StartupCheck {
            id: "authentication",
            label: "认证",
            status: "warning",
            detail: "等待 Grok 安装、版本和目录检查通过。".into(),
            code: None,
            action: None,
        };
        acp_check = skipped_acp_check();
    }
    checks.push(auth_check);
    checks.push(db_check);
    checks.push(workspace_check);
    checks.push(acp_check);

    let ready = checks
        .iter()
        .filter(|check| check.id != "git")
        .all(|check| check.status == "success");
    let actionable_error = checks
        .iter()
        .find(|check| check.status == "error")
        .map(|check| {
            let code = check
                .code
                .clone()
                .unwrap_or_else(|| "RUNTIME_CHECK_FAILED".into());
            ActionableRuntimeError {
                diagnostic: format!("[{}] {}", code, check.detail),
                code,
                message: check.detail.clone(),
                action: check.action.clone().unwrap_or_else(|| "重新检测。".into()),
            }
        });
    RuntimeReadinessSnapshot {
        installed,
        version: probe.version,
        min_version,
        authenticated,
        ready,
        checks,
        login: runtime.login_status().await,
        actionable_error,
    }
}

fn skipped_acp_check() -> StartupCheck {
    StartupCheck {
        id: "acp",
        label: "ACP 握手",
        status: "warning",
        detail: "等待认证和前置检查通过。".into(),
        code: None,
        action: None,
    }
}

fn is_authentication_failure(message: &str) -> bool {
    let safe = message.to_ascii_lowercase();
    safe.contains("authentication is required")
        || safe.contains("not authenticated")
        || safe.contains("authorizationrequired")
        || safe.contains("401")
}

enum AuthenticationVerification {
    Authenticated,
    AuthenticationRequired,
    Failed,
}

async fn verify_authentication_with_minimal_turn(
    runtime: &dyn AgentRuntime,
    session_id: &SessionId,
    events: &mut tokio::sync::mpsc::Receiver<TimestampedEvent>,
) -> AuthenticationVerification {
    if runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(PromptRequest {
                message: "Reply with exactly: OK".into(),
                attachments: vec![],
                mode: None,
                model: None,
                reasoning: None,
            }),
        )
        .await
        .is_err()
    {
        return AuthenticationVerification::Failed;
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return AuthenticationVerification::Failed;
        }
        let event = match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Some(event)) if event.meta.session_id == *session_id => event,
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => return AuthenticationVerification::Failed,
        };
        if let Some(verification) = terminal_authentication_verification(&event.event) {
            return verification;
        }
    }
}

fn terminal_authentication_verification(event: &AgentEvent) -> Option<AuthenticationVerification> {
    match event {
        // ACP reports request failures separately. A completed Turn therefore
        // proves that the service accepted the current credentials even when
        // the agent intentionally returns no user-visible text.
        AgentEvent::AssistantCompleted(_) => Some(AuthenticationVerification::Authenticated),
        AgentEvent::RequestFailed(failure) => Some(
            if failure.code == "GROK_AUTH_REQUIRED" || is_authentication_failure(&failure.message) {
                AuthenticationVerification::AuthenticationRequired
            } else {
                AuthenticationVerification::Failed
            },
        ),
        _ => None,
    }
}

async fn check_git() -> StartupCheck {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("--version")
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for key in [
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), command.status()).await {
        Ok(Ok(status)) if status.success() => StartupCheck {
            id: "git",
            label: "Git",
            status: "success",
            detail: "Git 可用。".into(),
            code: None,
            action: None,
        },
        _ => StartupCheck {
            id: "git",
            label: "Git",
            status: "warning",
            detail: "未检测到可用的 Git；普通对话仍可继续，但 Git/Worktree 功能不可用。".into(),
            code: Some(domain::error::codes::GIT_COMMAND_FAILED.into()),
            action: Some("安装 Git for Windows 后重新检测。".into()),
        },
    }
}

fn check_working_directory() -> (StartupCheck, Option<std::path::PathBuf>) {
    let Ok(directory) = std::env::current_dir() else {
        return (
            StartupCheck {
                id: "directory",
                label: "工作目录",
                status: "error",
                detail: "无法取得应用工作目录。".into(),
                code: Some("RUNTIME_DIRECTORY_UNAVAILABLE".into()),
                action: Some("从可访问目录重新启动应用。".into()),
            },
            None,
        );
    };
    let probe_path = directory.join(format!(".gag-write-check-{}.tmp", uuid::Uuid::new_v4()));
    let writable = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
        .is_ok();
    if writable {
        let _ = std::fs::remove_file(&probe_path);
        (
            StartupCheck {
                id: "directory",
                label: "工作目录",
                status: "success",
                detail: "工作目录可访问且可写。".into(),
                code: None,
                action: None,
            },
            Some(directory),
        )
    } else {
        (
            StartupCheck {
                id: "directory",
                label: "工作目录",
                status: "error",
                detail: "工作目录不可写。".into(),
                code: Some("RUNTIME_DIRECTORY_UNAVAILABLE".into()),
                action: Some("选择可写目录或修复目录权限后重启应用。".into()),
            },
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{terminal_authentication_verification, AuthenticationVerification};
    use crate::modules::agent_runtime::{events::AssistantCompletedPayload, AgentEvent};

    #[test]
    fn successful_empty_completion_proves_authentication() {
        let event = AgentEvent::AssistantCompleted(AssistantCompletedPayload { full_text: None });

        assert!(matches!(
            terminal_authentication_verification(&event),
            Some(AuthenticationVerification::Authenticated)
        ));
    }
}
