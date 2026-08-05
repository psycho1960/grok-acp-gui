// GAG-008: Conversation timeline domain view-models (Renderer-only).
// Domain DTOs originate from DesktopBridge TypedDesktopEvent / command results.

import type { SessionId, TaskId } from "../../bridge/types";

/** Session run status shown in header / composer capabilities. */
export type ConversationRunStatus =
  | "idle"
  | "running"
  | "waiting_permission"
  | "waiting_plan"
  | "cancelling"
  | "error"
  | "disconnected"
  | "offline";

export type ToolPhase =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type TimelineItemKind =
  | "user"
  | "assistant"
  | "thinking"
  | "tool"
  | "activity"
  | "error"
  | "system"
  | "permission"
  | "plan"
  | "artifact"
  | "unknown";

export interface RedactedField {
  /** Visible, already-redacted summary from backend. */
  summary: string;
  /** When true, UI must not offer "copy raw" without explicit policy. */
  redacted: boolean;
}

export interface ToolCallView {
  toolCallId: string;
  title: string;
  kind: string;
  phase: ToolPhase;
  startedAt?: string;
  endedAt?: string;
  durationMs?: number;
  locations: string[];
  input: RedactedField;
  result: RedactedField;
  exitCode?: number;
  /** Read-only batch fold key (e.g. explore). */
  foldGroup?: string;
  /** Structured details only when not redacted. */
  detailsSafe?: string;
}

export interface PermissionSlotView {
  requestId: string;
  toolCall: {
    toolCallId: string;
    title?: string;
    kind?: string;
    locations?: string[];
  };
  options: Array<{
    optionId: string;
    name: string;
    kind: "allow_once" | "allow_always" | "reject_once" | "reject_always";
  }>;
  expired?: boolean;
}

export interface PlanSlotView {
  status: string;
  detailSummary: string;
}

export interface ArtifactSlotView {
  artifactId: string;
  mimeType: string;
  displayName: string;
}

export interface TimelineItemBase {
  /** Stable UI id (not necessarily event seq). */
  id: string;
  kind: TimelineItemKind;
  /** Primary event seq that created or last updated this item. */
  seq: number;
  sessionId: SessionId;
  timestamp: string;
  /** Optional deep-link target. */
  eventKey: string;
}

export interface UserMessageItem extends TimelineItemBase {
  kind: "user";
  text: string;
  pending?: boolean;
  failed?: boolean;
  errorMessage?: string;
}

export interface AssistantMessageItem extends TimelineItemBase {
  kind: "assistant";
  text: string;
  streaming: boolean;
  frozen: boolean;
}

export interface ThinkingItem extends TimelineItemBase {
  kind: "thinking";
  summary: string;
  durationMs?: number;
  expanded: boolean;
}

export interface ToolItem extends TimelineItemBase {
  kind: "tool";
  tool: ToolCallView;
  expanded: boolean;
}

export interface ActivityItem extends TimelineItemBase {
  kind: "activity";
  activityKind: string;
  detail: string;
}

export interface ErrorItem extends TimelineItemBase {
  kind: "error";
  message: string;
  code?: string;
  retryable?: boolean;
}

export interface SystemItem extends TimelineItemBase {
  kind: "system";
  message: string;
}

export interface PermissionItem extends TimelineItemBase {
  kind: "permission";
  slot: PermissionSlotView;
}

export interface PlanItem extends TimelineItemBase {
  kind: "plan";
  slot: PlanSlotView;
}

export interface ArtifactItem extends TimelineItemBase {
  kind: "artifact";
  slot: ArtifactSlotView;
}

export interface UnknownItem extends TimelineItemBase {
  kind: "unknown";
  eventType: string;
  safeSummary: string;
}

export type TimelineItem =
  | UserMessageItem
  | AssistantMessageItem
  | ThinkingItem
  | ToolItem
  | ActivityItem
  | ErrorItem
  | SystemItem
  | PermissionItem
  | PlanItem
  | ArtifactItem
  | UnknownItem;

export interface TimelineCursor {
  /** Highest applied event seq for the active session. */
  lastSeq: number;
  /** Seq boundary of last full snapshot (history ends here). */
  snapshotSeq: number;
}

export interface ConversationTimelineView {
  items: TimelineItem[];
  cursor: TimelineCursor;
  status: ConversationRunStatus;
}

export interface ComposerCapabilities {
  canSend: boolean;
  canCancel: boolean;
  disabledReason?: string;
  bridgeOnline: boolean;
}

export interface ComposerView {
  draft: string;
  capabilities: ComposerCapabilities;
  sendError?: string;
}

export interface ConversationHeaderView {
  taskId: TaskId;
  title: string;
  status: ConversationRunStatus;
  attemptLabel?: string;
  sessionId?: SessionId;
}

export interface SessionTimelineSnapshot {
  taskId: TaskId;
  sessionId: SessionId;
  title: string;
  status: ConversationRunStatus;
  /** Last seq included in this snapshot. */
  cursor: number;
  events: import("../../bridge/types").TypedDesktopEvent[];
  /** Optional pre-built items for pure history without replaying deltas. */
  items?: TimelineItem[];
  attempt?: number;
}

export interface ConversationState {
  taskId: TaskId | null;
  sessionId: SessionId | null;
  title: string;
  status: ConversationRunStatus;
  attempt: number;
  cursor: TimelineCursor;
  /** Ordered timeline items. */
  items: TimelineItem[];
  /** Dedup set: `${sessionId}:${seq}` */
  seenKeys: Set<string>;
  /** tool_call_id → item id */
  toolIndex: Map<string, string>;
  /** Current streaming assistant item id, if any. */
  streamingAssistantId: string | null;
  /** Pending seq gap — UI should request refresh. */
  needsSnapshotRefresh: boolean;
  gapFromSeq: number | null;
  /** Out-of-order session events held until every preceding seq arrives. */
  pendingEvents: Map<number, import("../../bridge/types").TypedDesktopEvent>;
}

export type ConversationLoadState =
  | "idle"
  | "loading"
  | "ready"
  | "stale"
  | "error";
