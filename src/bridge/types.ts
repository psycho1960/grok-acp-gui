// GAG-003: DesktopBridge contract types shared between Renderer and Rust.
//
// Every type here has a serde-tagged Rust counterpart in
// src-tauri/src/bridge/.  The fixture round-trip tests in
// tests/bridge-contracts.test.ts catch drift.

// ---------------------------------------------------------------------------
// Branded ID types (opaque strings — Renderer never constructs these)
// ---------------------------------------------------------------------------

declare const TaskIdBrand: unique symbol;
declare const SessionIdBrand: unique symbol;
declare const ProjectIdBrand: unique symbol;
declare const CorrelationIdBrand: unique symbol;

export type TaskId = string & { [TaskIdBrand]: never };
export type SessionId = string & { [SessionIdBrand]: never };
export type ProjectId = string & { [ProjectIdBrand]: never };
export type CorrelationId = string & { [CorrelationIdBrand]: never };

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

export interface BootstrapStatus {
  productName: string;
  version: string;
  platform: string;
  ready: boolean;
}

// GAG-003 replaces GAG-001's BootstrapStatus with BootstrapSnapshot once
// deep modules are wired.  For now the shape is an alias.
export type BootstrapSnapshot = BootstrapStatus;

// ---------------------------------------------------------------------------
// DesktopCommand discriminated union
// ---------------------------------------------------------------------------

export type DesktopCommand =
  | { type: "runtime.refresh"; payload: Record<string, never> }
  | { type: "runtime.login"; payload: RuntimeLoginPayload }
  | { type: "project.open"; payload: ProjectOpenPayload }
  | { type: "project.forget"; payload: ProjectForgetPayload }
  | { type: "task.create"; payload: TaskCreatePayload }
  | { type: "task.open"; payload: TaskOpenPayload }
  | { type: "task.archive"; payload: TaskArchivePayload }
  | { type: "turn.send"; payload: TurnSendPayload }
  | { type: "turn.cancel"; payload: TurnCancelPayload }
  | { type: "session.configure"; payload: SessionConfigurePayload }
  | { type: "session.resume"; payload: SessionResumePayload }
  | { type: "permission.resolve"; payload: PermissionResolvePayload }
  | { type: "plan.resolve"; payload: PlanResolvePayload }
  | { type: "artifact.import"; payload: ArtifactImportPayload }
  | { type: "artifact.save"; payload: ArtifactSavePayload }
  | { type: "workspace.inspect"; payload: WorkspaceInspectPayload }
  | { type: "worktree.adopt"; payload: WorktreeAdoptPayload }
  | { type: "review.diff"; payload: ReviewDiffPayload }
  | { type: "review.checkpoint"; payload: ReviewCheckpointPayload }
  | { type: "integration.preflight"; payload: IntegrationPreflightPayload }
  | { type: "integration.execute"; payload: IntegrationExecutePayload }
  | { type: "worktree.cleanup"; payload: WorktreeCleanupPayload }
  | { type: "recovery.restore"; payload: RecoveryRestorePayload }
  | { type: "recovery.delete"; payload: RecoveryDeletePayload };

export interface RuntimeLoginPayload {
  method?: string;
}

export interface ProjectOpenPayload {
  path: string;
}

export interface ProjectForgetPayload {
  projectId: ProjectId;
}

export interface TaskCreatePayload {
  projectId: ProjectId;
  title: string;
  mode?: string;
  model?: string;
  reasoning?: string;
}

export interface TaskOpenPayload {
  taskId: TaskId;
}

export interface TaskArchivePayload {
  taskId: TaskId;
}

export interface TurnSendPayload {
  taskId: TaskId;
  message: string;
  attachments?: string[];
}

export interface TurnCancelPayload {
  taskId: TaskId;
}

export interface SessionConfigurePayload {
  taskId: TaskId;
  settings: Record<string, unknown>;
}

export interface SessionResumePayload {
  taskId: TaskId;
}

export interface PermissionResolvePayload {
  requestId: string;
  optionId: string;
}

export interface PlanResolvePayload {
  requestId: string;
  action: "approve" | "reject" | "keep_planning";
}

export interface ArtifactImportPayload {
  taskId: TaskId;
  paths: string[];
}

export interface ArtifactSavePayload {
  taskId: TaskId;
  artifactIds: string[];
  targetPath: string;
}

export interface WorkspaceInspectPayload {
  path: string;
}

export interface WorktreeAdoptPayload {
  path: string;
}

export interface ReviewDiffPayload {
  taskId: TaskId;
  paths?: string[];
}

export interface ReviewCheckpointPayload {
  taskId: TaskId;
  message: string;
  paths: string[];
}

export interface IntegrationPreflightPayload {
  taskId: TaskId;
}

export interface IntegrationExecutePayload {
  taskId: TaskId;
}

export interface WorktreeCleanupPayload {
  taskId: TaskId;
  force?: boolean;
}

export interface RecoveryRestorePayload {
  itemId: string;
}

export interface RecoveryDeletePayload {
  itemId: string;
}

// ---------------------------------------------------------------------------
// DesktopEvent
// ---------------------------------------------------------------------------

export interface DesktopEvent {
  type: string;
  taskId?: TaskId;
  sessionId?: SessionId;
  seq?: number;
  timestamp: string; // ISO 8601
  payload: unknown;
}

// ---------------------------------------------------------------------------
// DesktopResult
// ---------------------------------------------------------------------------

export type DesktopResult<T = unknown> =
  | { success: "true"; data: T }
  | { success: "false"; error: AppError };

// ---------------------------------------------------------------------------
// AppError
// ---------------------------------------------------------------------------

export interface AppError {
  code: string;
  message: string;
  action?: string;
  retryable: boolean;
  detailsRedacted: boolean;
  correlationId: CorrelationId;
}

// ---------------------------------------------------------------------------
// DesktopBridge Interface
// ---------------------------------------------------------------------------

export type Unsubscribe = () => void;

export interface DesktopBridge {
  bootstrap(): Promise<BootstrapSnapshot>;
  execute(command: DesktopCommand): Promise<DesktopResult>;
  subscribe(listener: (event: DesktopEvent) => void): Promise<Unsubscribe>;
}

// ---------------------------------------------------------------------------
// Well-known event types (mirrors Rust bridge::events::event_types)
// ---------------------------------------------------------------------------

export const EventTypes = {
  RUNTIME_UPDATED: "runtime.updated",
  TASK_SNAPSHOT: "task.snapshot",
  TASK_STATE: "task.state",
  MESSAGE_DELTA: "message.delta",
  ACTIVITY_UPDATED: "activity.updated",
  PERMISSION_REQUESTED: "permission.requested",
  PLAN_UPDATED: "plan.updated",
  CHANGES_UPDATED: "changes.updated",
  ARTIFACT_AVAILABLE: "artifact.available",
  RESOURCE_WARNING: "resource.warning",
  DIAGNOSTIC_NOTICE: "diagnostic.notice",
} as const;

// ---------------------------------------------------------------------------
// Well-known error codes (mirrors Rust domain::error::codes)
// ---------------------------------------------------------------------------

export const ErrorCodes = {
  RUNTIME_PROBE_FAILED: "RUNTIME_PROBE_FAILED",
  RUNTIME_NOT_FOUND: "RUNTIME_NOT_FOUND",
  RUNTIME_LOGIN_FAILED: "RUNTIME_LOGIN_FAILED",
  RUNTIME_PROCESS_DIED: "RUNTIME_PROCESS_DIED",
  ACP_HANDSHAKE_FAILED: "ACP_HANDSHAKE_FAILED",
  ACP_UNSUPPORTED_CAPABILITY: "ACP_UNSUPPORTED_CAPABILITY",
  ACP_REQUEST_FAILED: "ACP_REQUEST_FAILED",
  PROJECT_NOT_FOUND: "PROJECT_NOT_FOUND",
  PROJECT_ALREADY_EXISTS: "PROJECT_ALREADY_EXISTS",
  GIT_COMMAND_FAILED: "GIT_COMMAND_FAILED",
  GIT_LOCKED: "GIT_LOCKED",
  WORKTREE_ALREADY_EXISTS: "WORKTREE_ALREADY_EXISTS",
  WORKTREE_OUTSIDE_REPO: "WORKTREE_OUTSIDE_REPO",
  INTEGRATION_CONFLICT: "INTEGRATION_CONFLICT",
  INTEGRATION_DIRTY: "INTEGRATION_DIRTY",
  ARTIFACT_TOO_LARGE: "ARTIFACT_TOO_LARGE",
  ARTIFACT_INVALID_FORMAT: "ARTIFACT_INVALID_FORMAT",
  DB_MIGRATION_FAILED: "DB_MIGRATION_FAILED",
  DB_QUERY_FAILED: "DB_QUERY_FAILED",
  BRIDGE_UNSUPPORTED_COMMAND: "BRIDGE_UNSUPPORTED_COMMAND",
  BRIDGE_INVALID_PAYLOAD: "BRIDGE_INVALID_PAYLOAD",
  BRIDGE_VALIDATION_FAILED: "BRIDGE_VALIDATION_FAILED",
} as const;
