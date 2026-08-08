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

export interface BootstrapSnapshot {
  productName: string;
  version: string;
  platform: string;
  ready: boolean;
  /**
   * Set when the database is unavailable or corrupt. The Renderer must
   * show `UI-ERROR-001` and NOT render `ShellView`.
   * Mirrors Rust `BootstrapSnapshot::db_error`.
   */
  dbError?: string;
  runtime: RuntimeBootstrapStatus;
  capabilities: CapabilitySnapshot;
  // Domain entities (GAG-004) — may be empty when `ready=false`.
  projects?: Project[];
  activeTasks?: Task[];
  bindings?: SessionBinding[];
  worktrees?: WorktreeRecord[];
  recoveryItems?: RecoveryItem[];
  settings?: Settings[];
  recoveryPerformed?: boolean;
  tasksInterrupted?: number;
}

export interface RuntimeBootstrapStatus {
  status: "probing" | "ready" | "unavailable";
  /** Set when status is 'unavailable'. */
  probeError?: string;
  /** Runtime version string, if detected. */
  version?: string;
  /** Whether the user has authenticated with the runtime. */
  authenticated?: boolean;
}

export interface CapabilitySnapshot {
  /** Available models (ACP ModelInfo). */
  models: ModelInfo[];
  /** Session modes from ACP. */
  modes: ModeInfo[];
  /** Available slash commands (ACP AvailableCommand). */
  slashCommands: SlashCommandInfo[];
  /** Current model state, if active. */
  modelState?: SessionModelState;
  /** Current mode state, if active. */
  modeState?: SessionModeState;
}

export interface SessionModelState {
  currentModelId: string;
}

export interface SessionModeState {
  currentModeId: string;
}

export interface ModelInfo {
  /** ACP modelId. */
  modelId: string;
  /** Human-readable name. */
  name: string;
  /** Optional description. */
  description?: string;
  /** Default reasoning effort from the selected Grok config model profile. */
  reasoningEffort?: ReasoningEffort;
}

export type ReasoningEffort = "low" | "medium" | "high" | "max";

export interface ModeInfo {
  /** Mode identifier string. */
  id: string;
  /** Human-readable name. */
  name: string;
  /** Optional description. */
  description?: string;
}

export interface SlashCommandInfo {
  /** Command name (e.g. "create_plan"). */
  name: string;
  /** Human-readable description. */
  description: string;
  /** Whether the command accepts text input. */
  acceptsInput: boolean;
}

/** @deprecated Use BootstrapSnapshot instead. */
export type BootstrapStatus = BootstrapSnapshot;

// ---------------------------------------------------------------------------
// Domain entities (mirrors Rust `domain::types` — GAG-004)
// ---------------------------------------------------------------------------

export type TaskStatus =
  | "draft"
  | "preparing"
  | "running"
  | "waiting_permission"
  | "idle"
  | "failed"
  | "ready_for_review"
  | "integrating"
  | "conflicted"
  | "merged"
  | "archived"
  | "interrupted";

export type WorkspaceKind = "worktree" | "readonly" | "direct";

export type WorktreeOwnership = "managed" | "external";

export type WorktreeState =
  | "ready"
  | "dirty"
  | "integrating"
  | "deleted"
  | "unknown";

export type SessionState =
  | "active"
  | "idle"
  | "disconnected"
  | "closed";

export type RecoveryState =
  | "available"
  | "expired"
  | "restoring"
  | "restored"
  | "deleted";

export interface Project {
  id: ProjectId;
  path: string;
  displayPath: string;
  repoRoot?: string;
  trustedAt?: string;
  lastOpenedAt: string;
}

export interface Task {
  id: TaskId;
  projectId: ProjectId;
  title: string;
  status: TaskStatus;
  workspaceKind: WorkspaceKind;
  mode?: string;
  model?: string;
  reasoning?: string;
  createdAt: string;
  updatedAt: string;
  interruptReason?: string;
}

export interface SessionBinding {
  taskId: TaskId;
  sessionId: SessionId;
  cwd?: string;
  lastSeq: number;
  state: SessionState;
}

export interface WorktreeRecord {
  id: string;
  taskId: TaskId;
  repoRoot: string;
  path: string;
  displayPath: string;
  branch: string;
  baseBranch: string;
  baseCommit: string;
  ownership: WorktreeOwnership;
  state: WorktreeState;
}

export interface RecoveryItem {
  id: string;
  taskId: TaskId;
  directory: string;
  manifestPath: string;
  expiresAt: string;
  state: RecoveryState;
}

export interface Settings {
  key: string;
  jsonValue: unknown;
}

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
  | { type: "artifact.import.blob"; payload: ArtifactImportBlobPayload }
  | { type: "artifact.list"; payload: ArtifactListPayload }
  | { type: "artifact.preview"; payload: ArtifactIdPayload }
  | { type: "artifact.reveal"; payload: ArtifactIdPayload }
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
  /** Optional — when empty the backend derives it from the prompt's first sentence. */
  title?: string;
  /** Initial prompt text (FR-TASK-001). Required. */
  prompt: string;
  /** Attachments referenced by artifact ID. */
  attachments?: string[];
  /** Agent mode string (ACP SessionModeId — dynamic). */
  mode?: string;
  /** Model ID to use for this task. */
  model?: string;
  /** Reasoning effort from the selected Grok config model profile. */
  reasoning?: ReasoningEffort;
  /** Workspace strategy: "worktree" | "readonly" | "direct". */
  workspaceStrategy?: "worktree" | "readonly" | "direct";
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
  taskId: TaskId;
  sessionId: SessionId;
  requestId: string;
  correlationId: string;
  /** Zero when the request is not bound to a Plan. */
  expectedVersion: number;
  optionId: string;
}

export interface PlanResolvePayload {
  taskId: TaskId;
  sessionId: SessionId;
  requestId: string;
  correlationId: string;
  expectedVersion: number;
  /** ACP option ID, passed verbatim from the permission request. */
  optionId: string;
}

export interface ArtifactImportPayload {
  taskId: TaskId;
  paths: string[];
}

/** One clipboard image blob (no filesystem path) submitted by the Renderer. */
export interface ArtifactBlobInput {
  /** Display name used for the managed attachment (e.g. "截图.png"). */
  displayName: string;
  /** Base64-encoded image bytes. */
  base64Data: string;
}

export interface ArtifactImportBlobPayload {
  taskId: TaskId;
  blobs: ArtifactBlobInput[];
}

export interface ArtifactListPayload { taskId: TaskId; }
export interface ArtifactIdPayload { taskId: TaskId; artifactId: string; }

/** Metadata-only Artifact DTO. Cache paths and bytes never cross DesktopBridge. */
export interface ArtifactDescriptor {
  artifactId: string;
  displayName: string;
  mimeType: string;
  bytes: number;
  state: "ready" | "rejected" | "failed" | "missing" | "quarantined" | string;
  previewCapability: "inline" | "onDemand" | "none" | string;
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

/**
 * Discriminated union mapping event type → typed payload.
 * Use this for type-safe event handling in reducers / stores.
 * Session events (marked with *) require taskId/sessionId/seq.
 */
export type TypedDesktopEvent =
  // non-session events
  | { type: "runtime.updated"; timestamp: string; payload: RuntimeUpdatedPayload }
  | { type: "resource.warning"; timestamp: string; payload: ResourceWarningPayload }
  | { type: "diagnostic.notice"; timestamp: string; payload: DiagnosticNoticePayload }
  // session-scoped events
  | { type: "task.snapshot"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: TaskSnapshotPayload }
  | { type: "task.state"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: TaskStatePayload }
  | { type: "message.delta"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: MessageDeltaPayload }
  | { type: "activity.updated"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: ActivityUpdatedPayload }
  | { type: "permission.requested"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: PermissionRequestedPayload }
  | { type: "plan.updated"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: PlanUpdatedPayload }
  | { type: "changes.updated"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: ChangesUpdatedPayload }
  | { type: "artifact.available"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: ArtifactAvailablePayload }
  | { type: "session.commands.updated"; taskId: TaskId; sessionId: SessionId; seq: number; timestamp: string; payload: SessionCommandsUpdatedPayload };

/**
 * Session-scoped event envelope with required taskId/sessionId/seq.
 * Convert to DesktopEvent via `build()` for emission.
 */
export interface SessionEvent {
  type: string;
  taskId: TaskId;
  sessionId: SessionId;
  seq: number;
  payload: unknown;
}

// ---------------------------------------------------------------------------
// Typed event payloads (mirrors Rust bridge::events payload structs)
// ---------------------------------------------------------------------------

export interface RuntimeUpdatedPayload {
  status: string;
}

export interface TaskSnapshotPayload {
  tasks: unknown; // typed in GAG-004
}

export interface TaskStatePayload {
  taskId: TaskId;
  status: string;
  detail: unknown;
}

export interface MessageDeltaPayload {
  role?: "user" | "assistant";
  text?: string;
  toolCall?: unknown;
}

export interface ActivityUpdatedPayload {
  kind: string;
  detail: string;
  code?: string;
  retryable?: boolean;
}

export interface PermissionRequestedPayload {
  requestId: string;
  correlationId: string;
  expectedVersion: number | null;
  expiresAtEpochSeconds: number;
  options: PermissionOption[];
  /** ACP ToolCallUpdate summary for the UI. */
  toolCall: ToolCallSummary;
  operation: PermissionOperationView;
}

export interface PermissionOperationView {
  category: "read_only" | "write" | "destructive" | "unknown";
  executable?: string;
  args?: string[];
  cwd?: string;
  readPaths?: string[];
  writePaths?: string[];
  risk: string;
}

export interface ToolCallSummary {
  toolCallId: string;
  title?: string;
  kind?: string;
  locations?: string[];
}

export interface PermissionOption {
  /** ACP optionId — passed verbatim. */
  optionId: string;
  /** Human-readable label. */
  name: string;
  /** Explicit ACP semantic field. Unknown values remain unknown. */
  kind?: string;
}

export interface PlanUpdatedPayload {
  status: string;
  detail: {
    requestId?: string;
    correlationId?: string;
    version?: number;
    summary?: string;
    steps?: string[];
    options?: PermissionOption[];
    reason?: string;
  };
}

export interface ChangesUpdatedPayload {
  taskId: TaskId;
  files: unknown;
}

export interface ArtifactAvailablePayload {
  taskId: TaskId;
  artifactId: string;
  mimeType: string;
  displayName: string;
  state?: "ready" | "quarantined" | "missing" | string;
}

/** The ACP session published or changed its slash commands. */
export interface SessionCommandsUpdatedPayload {
  commands: SlashCommandInfo[];
}

export interface ResourceWarningPayload {
  message: string;
  resource: string;
}

export interface DiagnosticNoticePayload {
  level: string;
  message: string;
  source: string;
}

// ---------------------------------------------------------------------------
// DesktopResult
// ---------------------------------------------------------------------------

export type DesktopResult<T = unknown> =
  | { success: "true"; data: T }
  | { success: "false"; error: AppError };

/** Typed result DTOs mapped to each command category. */
export interface RuntimeStatusResult {
  status: string;
  version?: string;
}

export interface ProjectOpenResult {
  projectId: ProjectId;
}

export interface TaskCreateResult {
  taskId: TaskId;
}

export interface TaskOpenResult {
  taskId: TaskId;
  title: string;
  status: string;
  /** Persisted model selection (restored for the conversation controls). */
  model?: string | null;
  /** Persisted reasoning effort selection. */
  reasoning?: ReasoningEffort | string | null;
  sessionId?: SessionId;
  cursor?: number | { lastSeq?: number; snapshotSeq?: number };
  events?: TypedDesktopEvent[];
  attempt?: number;
}

export interface TurnSendResult {
  requestId?: number;
  /** Compatibility with the original mock response. */
  seq?: number;
}

export interface ArtifactImportResult {
  artifacts: ArtifactDescriptor[];
}

export interface ArtifactListResult {
  artifacts: ArtifactDescriptor[];
}

export interface ArtifactPreviewResult {
  artifact: ArtifactDescriptor;
  /** Opaque custom-scheme URL. Never a filesystem path or data URL. */
  url: string;
}

export interface WorkspaceInspectResult {
  repoRoot: string;
  branch: string;
  dirty: boolean;
}

export interface ReviewDiffResult {
  files: FileDiffSummary[];
}

export interface FileDiffSummary {
  path: string;
  status: "added" | "modified" | "deleted" | "renamed";
}

export interface AcknowledgedResult {
  acknowledged: string;
}

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
  subscribe(listener: (event: TypedDesktopEvent) => void): Promise<Unsubscribe>;
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
  SESSION_COMMANDS_UPDATED: "session.commands.updated",
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
  BRIDGE_NOT_IMPLEMENTED: "BRIDGE_NOT_IMPLEMENTED",
} as const;
