// GAG-008: Snapshot + delta merge reducer for conversation timeline.
// Dedup: (sessionId, seq). Tool merge by toolCallId. No completed→running.

import type {
  SessionId,
  TaskId,
  TypedDesktopEvent,
} from "../../bridge/types";
import { mergeToolCall, normalizeToolCall } from "./tool-normalize";
import type {
  ActivityItem,
  ArtifactItem,
  AssistantMessageItem,
  ConversationRunStatus,
  ConversationState,
  ErrorItem,
  PermissionItem,
  PlanItem,
  SessionTimelineSnapshot,
  SystemItem,
  TimelineItem,
  ToolItem,
  UnknownItem,
  UserMessageItem,
} from "./types";

function eventKey(sessionId: SessionId | string, seq: number): string {
  return `${sessionId}:${seq}`;
}

function itemId(kind: string, seq: number, extra = ""): string {
  return extra ? `${kind}-${seq}-${extra}` : `${kind}-${seq}`;
}

const VISUAL_CONTEXT_MARKER =
  '<attachment_visual_context source="gpt-5.6-luna" trust="untrusted">';

/**
 * The main runtime receives Luna OCR in a private prompt suffix. Some ACP
 * servers echo that composed prompt as a user_message_chunk; only the text the
 * user actually entered may be rendered or used to reconcile optimistic UI.
 */
export function stripInternalVisualContext(text: string): string {
  const markerIndex = text.indexOf(VISUAL_CONTEXT_MARKER);
  if (markerIndex < 0) return text;
  return text.slice(0, markerIndex).trimEnd();
}

export function createEmptyConversationState(
  taskId: TaskId | null = null,
): ConversationState {
  return {
    taskId,
    sessionId: null,
    title: "",
    status: "idle",
    attempt: 1,
    cursor: { lastSeq: 0, snapshotSeq: 0 },
    items: [],
    seenKeys: new Set(),
    toolIndex: new Map(),
    streamingAssistantId: null,
    needsSnapshotRefresh: false,
    gapFromSeq: null,
    pendingEvents: new Map(),
  };
}

function mapTaskStatus(status: string): ConversationRunStatus {
  switch (status) {
    case "running":
    case "preparing":
      return "running";
    case "waiting_permission":
      return "waiting_permission";
    case "failed":
    case "interrupted":
    case "conflicted":
      return "error";
    case "integrating":
      return "running";
    case "merged":
    case "archived":
      return "idle";
    default:
      return "idle";
  }
}

function freezeStreamingAssistant(state: ConversationState): ConversationState {
  if (!state.streamingAssistantId) return state;
  const items = state.items.map((it) => {
    if (it.id !== state.streamingAssistantId || it.kind !== "assistant") return it;
    return { ...it, streaming: false, frozen: true };
  });
  return { ...state, items, streamingAssistantId: null };
}

function upsertItem(state: ConversationState, item: TimelineItem): ConversationState {
  const idx = state.items.findIndex((i) => i.id === item.id);
  if (idx < 0) {
    return { ...state, items: [...state.items, item] };
  }
  const items = state.items.slice();
  items[idx] = item;
  return { ...state, items };
}

function replaceItem(
  state: ConversationState,
  id: string,
  next: TimelineItem,
): ConversationState {
  return {
    ...state,
    items: state.items.map((i) => (i.id === id ? next : i)),
  };
}

function markSeen(
  state: ConversationState,
  sessionId: SessionId,
  seq: number,
): ConversationState {
  const key = eventKey(sessionId, seq);
  if (state.seenKeys.has(key)) return state;
  const seenKeys = new Set(state.seenKeys);
  seenKeys.add(key);
  const lastSeq = Math.max(state.cursor.lastSeq, seq);
  // Gap detection: seq should be lastSeq+1 for continuous stream after snapshot
  let needsSnapshotRefresh = state.needsSnapshotRefresh;
  let gapFromSeq = state.gapFromSeq;
  if (
    state.cursor.lastSeq > 0 &&
    seq > state.cursor.lastSeq + 1 &&
    seq > state.cursor.snapshotSeq
  ) {
    needsSnapshotRefresh = true;
    gapFromSeq = state.cursor.lastSeq + 1;
  }
  return {
    ...state,
    seenKeys,
    cursor: { ...state.cursor, lastSeq },
    needsSnapshotRefresh,
    gapFromSeq,
  };
}

function applyMessageDelta(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "message.delta" }>,
): ConversationState {
  let next = markSeen(state, event.sessionId, event.seq);
  const { role, text, toolCall } = event.payload;

  if (role === "user" && typeof text === "string" && text.length > 0) {
    const visibleText = stripInternalVisualContext(text);
    const optimistic = [...next.items]
      .reverse()
      .find(
        (item): item is UserMessageItem =>
          item.kind === "user" &&
          item.text === visibleText &&
          item.eventKey.startsWith("user:") &&
          !item.failed,
      );
    const confirmed: UserMessageItem = {
      id: optimistic?.id ?? itemId("user", event.seq),
      kind: "user",
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      text: visibleText,
      attachments: optimistic?.attachments,
      pending: false,
      failed: false,
      errorMessage: undefined,
    };
    return optimistic
      ? replaceItem(next, optimistic.id, confirmed)
      : upsertItem(next, confirmed);
  }

  if (toolCall != null) {
    const normalized = normalizeToolCall(toolCall);
    if (!normalized) {
      const unknown: UnknownItem = {
        id: itemId("unknown", event.seq),
        kind: "unknown",
        seq: event.seq,
        sessionId: event.sessionId,
        timestamp: event.timestamp,
        eventKey: eventKey(event.sessionId, event.seq),
        eventType: "message.delta.toolCall",
        safeSummary: "无法解析的工具事件",
      };
      return upsertItem(next, unknown);
    }

    const existingId = next.toolIndex.get(normalized.toolCallId);
    if (existingId) {
      const existing = next.items.find((i) => i.id === existingId);
      if (existing && existing.kind === "tool") {
        const merged = mergeToolCall(existing.tool, normalized);
        const toolItem: ToolItem = {
          ...existing,
          seq: event.seq,
          timestamp: event.timestamp,
          tool: merged,
        };
        return replaceItem(next, existingId, toolItem);
      }
    }

    const id = itemId("tool", event.seq, normalized.toolCallId);
    const toolItem: ToolItem = {
      id,
      kind: "tool",
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      tool: normalized,
      expanded: false,
    };
    const toolIndex = new Map(next.toolIndex);
    toolIndex.set(normalized.toolCallId, id);
    next = { ...next, toolIndex };
    return upsertItem(next, toolItem);
  }

  if (typeof text === "string" && text.length > 0) {
    if (next.streamingAssistantId) {
      const existing = next.items.find((i) => i.id === next.streamingAssistantId);
      if (existing && existing.kind === "assistant" && !existing.frozen) {
        const updated: AssistantMessageItem = {
          ...existing,
          text: existing.text + text,
          seq: event.seq,
          timestamp: event.timestamp,
          streaming: true,
          frozen: false,
        };
        return replaceItem(next, existing.id, updated);
      }
    }

    const id = itemId("assistant", event.seq);
    const assistant: AssistantMessageItem = {
      id,
      kind: "assistant",
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      text,
      streaming: true,
      frozen: false,
    };
    next = { ...next, streamingAssistantId: id };
    return upsertItem(next, assistant);
  }

  return next;
}

function applyActivity(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "activity.updated" }>,
): ConversationState {
  let next = markSeen(state, event.sessionId, event.seq);
  const { kind, detail, code, retryable } = event.payload;

  if (kind === "thinking" || kind === "thought") {
    const id = itemId("thinking", event.seq);
    const thinking = {
      id,
      kind: "thinking" as const,
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      summary: detail || "Thinking…",
      expanded: false,
    };
    return upsertItem(next, thinking);
  }

  if (kind === "error" || kind === "failed") {
    const err: ErrorItem = {
      id: itemId("error", event.seq),
      kind: "error",
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      message: detail || "发生错误",
      code,
      retryable: retryable !== false,
    };
    next = freezeStreamingAssistant(next);
    next = { ...next, status: "error" };
    return upsertItem(next, err);
  }

  const activity: ActivityItem = {
    id: itemId("activity", event.seq),
    kind: "activity",
    seq: event.seq,
    sessionId: event.sessionId,
    timestamp: event.timestamp,
    eventKey: eventKey(event.sessionId, event.seq),
    activityKind: kind,
    detail,
  };
  return upsertItem(next, activity);
}

function applyTaskState(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "task.state" }>,
): ConversationState {
  let next = markSeen(state, event.sessionId, event.seq);
  const status = mapTaskStatus(event.payload.status);
  next = { ...next, status, taskId: event.taskId, sessionId: event.sessionId };

  if (status === "idle" || status === "error" || status === "waiting_permission") {
    next = freezeStreamingAssistant(next);
  }

  // Terminal interrupt → system line
  if (event.payload.status === "interrupted") {
    const sys: SystemItem = {
      id: itemId("system", event.seq),
      kind: "system",
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      message: "任务已中断",
    };
    next = upsertItem(next, sys);
  }

  const detail = event.payload.detail;
  const cancelled =
    detail != null &&
    typeof detail === "object" &&
    "reason" in detail &&
    detail.reason === "cancelled";
  if (cancelled) {
    next = {
      ...next,
      items: next.items.map((item) =>
        item.kind === "tool" &&
        (item.tool.phase === "pending" || item.tool.phase === "running")
          ? { ...item, tool: { ...item.tool, phase: "cancelled" as const } }
          : item,
      ),
    };
    const stopped: SystemItem = {
      id: itemId("system", event.seq, "cancelled"),
      kind: "system",
      seq: event.seq,
      sessionId: event.sessionId,
      timestamp: event.timestamp,
      eventKey: eventKey(event.sessionId, event.seq),
      message: "已停止",
    };
    next = upsertItem(next, stopped);
  }

  return next;
}

function applyPermission(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "permission.requested" }>,
): ConversationState {
  let next = markSeen(state, event.sessionId, event.seq);
  next = {
    ...next,
    status: "waiting_permission",
  };
  next = freezeStreamingAssistant(next);

  const p = event.payload;
  const item: PermissionItem = {
    id: itemId("permission", event.seq, p.requestId),
    kind: "permission",
    seq: event.seq,
    sessionId: event.sessionId,
    timestamp: event.timestamp,
    eventKey: eventKey(event.sessionId, event.seq),
    slot: {
      taskId: event.taskId,
      sessionId: event.sessionId,
      requestId: p.requestId,
      correlationId: p.correlationId,
      expectedVersion: p.expectedVersion ?? 0,
      expiresAtEpochSeconds: p.expiresAtEpochSeconds,
      toolCall: {
        toolCallId: p.toolCall.toolCallId,
        title: p.toolCall.title,
        kind: p.toolCall.kind,
        locations: p.toolCall.locations,
      },
      options: p.options.map((o) => ({
        optionId: o.optionId,
        name: o.name,
        kind: o.kind,
      })),
      operation: p.operation,
      decisionState: "pending",
    },
  };
  return upsertItem(next, item);
}

function applyPlan(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "plan.updated" }>,
): ConversationState {
  let next = markSeen(state, event.sessionId, event.seq);
  const detail = event.payload.detail;
  let detailSummary = "Plan 已更新";
  if (typeof detail === "string") detailSummary = detail;
  else if (detail && typeof detail === "object") {
    detailSummary = detail.summary ?? "规划步骤已更新（详情见 Plan 面板）";
  }

  const status = event.payload.status;
  if (
    status === "awaiting_approval" ||
    status === "waiting" ||
    status === "pending" ||
    status === "proposed"
  ) {
    next = { ...next, status: "waiting_plan" };
  }

  const version = detail.version ?? 0;
  next = {
    ...next,
    items: next.items.map((item) =>
      item.kind === "plan" && item.slot.version < version
        ? {
            ...item,
            slot: {
              ...item.slot,
              approvalInvalidated: true,
              status: "superseded",
            },
          }
        : item,
    ),
  };
  const item: PlanItem = {
    id: itemId("plan", event.seq),
    kind: "plan",
    seq: event.seq,
    sessionId: event.sessionId,
    timestamp: event.timestamp,
    eventKey: eventKey(event.sessionId, event.seq),
    slot: {
      taskId: event.taskId,
      sessionId: event.sessionId,
      requestId: detail.requestId ?? "",
      correlationId: detail.correlationId ?? "",
      version,
      status,
      detailSummary,
      steps: detail.steps ?? [],
      options: detail.options ?? [],
      decisionState: "pending",
    },
  };
  return upsertItem(next, item);
}

export function updateApprovalDecision(
  state: ConversationState,
  itemId: string,
  update: {
    decisionState: "submitting" | "resolved" | "error";
    optionId?: string;
    errorMessage?: string;
    status?: string;
  },
): ConversationState {
  return {
    ...state,
    status: update.decisionState === "resolved" ? "running" : state.status,
    items: state.items.map((item) => {
      if (item.id !== itemId || (item.kind !== "permission" && item.kind !== "plan")) {
        return item;
      }
      return {
        ...item,
        slot: {
          ...item.slot,
          decisionState: update.decisionState,
          selectedOptionId: update.optionId,
          errorMessage: update.errorMessage,
          ...(item.kind === "plan" && update.status ? { status: update.status } : {}),
        },
      } as TimelineItem;
    }),
  };
}

function applyArtifact(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "artifact.available" }>,
): ConversationState {
  const next = markSeen(state, event.sessionId, event.seq);
  const p = event.payload;
  const item: ArtifactItem = {
    id: itemId("artifact", event.seq, p.artifactId),
    kind: "artifact",
    seq: event.seq,
    sessionId: event.sessionId,
    timestamp: event.timestamp,
    eventKey: eventKey(event.sessionId, event.seq),
    slot: {
      artifactId: p.artifactId,
      mimeType: p.mimeType,
      displayName: p.displayName,
    },
  };
  return upsertItem(next, item);
}

function applyChanges(
  state: ConversationState,
  event: Extract<TypedDesktopEvent, { type: "changes.updated" }>,
): ConversationState {
  const next = markSeen(state, event.sessionId, event.seq);
  const files = event.payload.files;
  const count = Array.isArray(files) ? files.length : 0;
  const activity: ActivityItem = {
    id: itemId("activity", event.seq, "changes"),
    kind: "activity",
    seq: event.seq,
    sessionId: event.sessionId,
    timestamp: event.timestamp,
    eventKey: eventKey(event.sessionId, event.seq),
    activityKind: "changes",
    detail: count > 0 ? `${count} 个文件变更` : "工作区变更已更新",
  };
  return upsertItem(next, activity);
}

function applyUnknownSessionEvent(
  state: ConversationState,
  event: TypedDesktopEvent & { sessionId: SessionId; seq: number },
): ConversationState {
  const next = markSeen(state, event.sessionId, event.seq);
  const item: UnknownItem = {
    id: itemId("unknown", event.seq),
    kind: "unknown",
    seq: event.seq,
    sessionId: event.sessionId,
    timestamp: event.timestamp,
    eventKey: eventKey(event.sessionId, event.seq),
    eventType: event.type,
    safeSummary: `未知事件：${event.type}`,
  };
  return upsertItem(next, item);
}

function applySequencedEvent(
  state: ConversationState,
  event: TypedDesktopEvent,
): ConversationState {
  if (!("sessionId" in event) || event.sessionId == null || event.seq == null) {
    return state;
  }
  const sessionId = event.sessionId;
  const seq = event.seq;
  const next: ConversationState = {
    ...state,
    sessionId: state.sessionId ?? sessionId,
    taskId: state.taskId ?? event.taskId ?? null,
  };

  switch (event.type) {
    case "message.delta":
      return applyMessageDelta(next, event);
    case "activity.updated":
      return applyActivity(next, event);
    case "task.state":
      return applyTaskState(next, event);
    case "permission.requested":
      return applyPermission(next, event);
    case "plan.updated":
      return applyPlan(next, event);
    case "artifact.available":
      return applyArtifact(next, event);
    case "changes.updated":
      return applyChanges(next, event);
    case "task.snapshot":
      // Full task list snapshots are handled at store layer; ignore here.
      return markSeen(next, sessionId, seq);
    default:
      return applyUnknownSessionEvent(
        next,
        event as TypedDesktopEvent & { sessionId: SessionId; seq: number },
      );
  }
}

/**
 * Apply a single DesktopEvent. Session events are rendered only in strict seq
 * order; future events are buffered and exact duplicates are ignored.
 */
export function applyEvent(
  state: ConversationState,
  event: TypedDesktopEvent,
): ConversationState {
  // Non-session diagnostics — only surface as system if we have an open session
  if (event.type === "diagnostic.notice") {
    if (!state.sessionId) return state;
    const item: SystemItem = {
      id: itemId("system", state.cursor.lastSeq, `diag-${event.timestamp}`),
      kind: "system",
      seq: state.cursor.lastSeq,
      sessionId: state.sessionId,
      timestamp: event.timestamp,
      eventKey: `diag:${event.timestamp}`,
      message: event.payload.message,
    };
    return upsertItem(state, item);
  }

  if (event.type === "resource.warning") {
    if (!state.sessionId) return state;
    const item: SystemItem = {
      id: itemId("system", state.cursor.lastSeq, `res-${event.timestamp}`),
      kind: "system",
      seq: state.cursor.lastSeq,
      sessionId: state.sessionId,
      timestamp: event.timestamp,
      eventKey: `res:${event.timestamp}`,
      message: event.payload.message,
    };
    return upsertItem(state, item);
  }

  if (event.type === "runtime.updated") {
    if (event.payload.status === "unavailable") {
      return { ...state, status: "offline" };
    }
    if (state.status === "offline" || state.status === "disconnected") {
      return { ...state, status: "idle" };
    }
    return state;
  }

  if (!("sessionId" in event) || event.sessionId == null || event.seq == null) {
    return state;
  }
  if (state.taskId && event.taskId && event.taskId !== state.taskId) {
    return state;
  }
  if (state.sessionId && event.sessionId !== state.sessionId) {
    return state;
  }

  const key = eventKey(event.sessionId, event.seq);
  if (state.seenKeys.has(key) || state.pendingEvents.has(event.seq)) {
    return state;
  }

  const bound: ConversationState = {
    ...state,
    sessionId: state.sessionId ?? event.sessionId,
    taskId: state.taskId ?? event.taskId ?? null,
  };
  const expected = bound.cursor.lastSeq + 1;
  if (event.seq > expected) {
    const pendingEvents = new Map(bound.pendingEvents);
    pendingEvents.set(event.seq, event);
    return {
      ...bound,
      pendingEvents,
      needsSnapshotRefresh: true,
      gapFromSeq: expected,
    };
  }
  if (event.seq < expected) {
    return {
      ...bound,
      needsSnapshotRefresh: true,
      gapFromSeq: Math.min(bound.gapFromSeq ?? event.seq, event.seq),
    };
  }

  let next = applySequencedEvent(bound, event);
  const pendingEvents = new Map(next.pendingEvents);
  let queued = pendingEvents.get(next.cursor.lastSeq + 1);
  while (queued) {
    pendingEvents.delete(next.cursor.lastSeq + 1);
    next = applySequencedEvent({ ...next, pendingEvents }, queued);
    queued = pendingEvents.get(next.cursor.lastSeq + 1);
  }

  const hasGap = pendingEvents.size > 0;
  return {
    ...next,
    pendingEvents,
    needsSnapshotRefresh: hasGap,
    gapFromSeq: hasGap ? next.cursor.lastSeq + 1 : null,
  };
}

/** Apply many events in order (still enforces dedup / gap rules). */
export function applyEvents(
  state: ConversationState,
  events: TypedDesktopEvent[],
): ConversationState {
  return events.reduce((s, e) => applyEvent(s, e), state);
}

/**
 * Apply a session timeline snapshot. Replaces history at/under cursor and
 * clears streaming state. Events in snapshot are replayed for item rebuild
 * unless `items` is provided.
 */
export function applySnapshot(
  _state: ConversationState,
  snapshot: SessionTimelineSnapshot,
): ConversationState {
  let next = createEmptyConversationState(snapshot.taskId);
  next = {
    ...next,
    sessionId: snapshot.sessionId,
    title: snapshot.title,
    status: snapshot.status,
    attempt: snapshot.attempt ?? 1,
    cursor: {
      lastSeq: 0,
      snapshotSeq: 0,
    },
    needsSnapshotRefresh: false,
    gapFromSeq: null,
    pendingEvents: new Map(),
  };

  if (snapshot.items && snapshot.items.length > 0) {
    // Trust prebuilt items; still register seen keys up to cursor
    const seenKeys = new Set<string>();
    const toolIndex = new Map<string, string>();
    for (const item of snapshot.items) {
      seenKeys.add(eventKey(item.sessionId, item.seq));
      if (item.kind === "tool") {
        toolIndex.set(item.tool.toolCallId, item.id);
      }
    }
    // Fill seq range so deltas with seq <= cursor are ignored as duplicates
    for (let s = 1; s <= snapshot.cursor; s++) {
      seenKeys.add(eventKey(snapshot.sessionId, s));
    }
    next = {
      ...next,
      items: snapshot.items.map((i) =>
        i.kind === "assistant" ? { ...i, streaming: false, frozen: true } : i,
      ),
      seenKeys,
      toolIndex,
      streamingAssistantId: null,
      pendingEvents: new Map(),
    };
    return next;
  }

  // A snapshot is authoritative history; replay it by seq even if persistence
  // returned rows in an unexpected order.
  const orderedEvents = [...snapshot.events].sort((left, right) => {
    const leftSeq = "seq" in left && typeof left.seq === "number" ? left.seq : 0;
    const rightSeq = "seq" in right && typeof right.seq === "number" ? right.seq : 0;
    return leftSeq - rightSeq;
  });
  // Snapshot rows may be compacted (for example thousands of consecutive
  // assistant deltas become one safe message event), so their original ACP
  // sequence numbers are intentionally sparse. The snapshot cursor is the
  // authoritative gap boundary; strict contiguous sequencing is only for
  // live deltas received after this snapshot.
  next = orderedEvents.reduce((state, event) => {
    if (!("sessionId" in event) || event.sessionId == null || event.seq == null) {
      return state;
    }
    return applySequencedEvent(state, event);
  }, next);
  next = freezeStreamingAssistant(next);
  next = {
    ...next,
    cursor: {
      lastSeq: Math.max(next.cursor.lastSeq, snapshot.cursor),
      snapshotSeq: snapshot.cursor,
    },
    title: snapshot.title,
    status: snapshot.status,
    attempt: snapshot.attempt ?? next.attempt,
    needsSnapshotRefresh: false,
    gapFromSeq: null,
  };
  // Ensure all seq ≤ cursor marked seen so late history deltas don't re-apply
  const seenKeys = new Set(next.seenKeys);
  for (let s = 1; s <= snapshot.cursor; s++) {
    seenKeys.add(eventKey(snapshot.sessionId, s));
  }
  next = { ...next, seenKeys };
  return next;
}

/** Local optimistic user message after send is accepted. */
export function appendUserMessage(
  state: ConversationState,
  text: string,
  opts: {
    id?: string;
    pending?: boolean;
    timestamp?: string;
    attachments?: UserMessageItem["attachments"];
  } = {},
): ConversationState {
  if (!state.sessionId && !state.taskId) {
    // Still allow local message before session binds
  }
  const sessionId = state.sessionId ?? ("pending" as SessionId);
  const timestamp = opts.timestamp ?? new Date().toISOString();
  const id = opts.id ?? `user-local-${timestamp}`;
  const item: UserMessageItem = {
    id,
    kind: "user",
    seq: state.cursor.lastSeq,
    sessionId,
    timestamp,
    eventKey: `user:${id}`,
    text,
    attachments: opts.attachments?.map((attachment) => ({ ...attachment })),
    pending: opts.pending ?? false,
  };
  return upsertItem(state, item);
}

export function markUserMessageFailed(
  state: ConversationState,
  id: string,
  errorMessage: string,
): ConversationState {
  const items = state.items.map((it) => {
    if (it.id !== id || it.kind !== "user") return it;
    return {
      ...it,
      pending: false,
      failed: true,
      errorMessage,
    };
  });
  return { ...state, items };
}

export function markUserMessageConfirmed(
  state: ConversationState,
  id: string,
): ConversationState {
  const items = state.items.map((it) => {
    if (it.id !== id || it.kind !== "user") return it;
    return { ...it, pending: false, failed: false, errorMessage: undefined };
  });
  return { ...state, items };
}

export function toggleToolExpanded(
  state: ConversationState,
  itemId: string,
): ConversationState {
  const items = state.items.map((it) => {
    if (it.id !== itemId || it.kind !== "tool") return it;
    return { ...it, expanded: !it.expanded };
  });
  return { ...state, items };
}

export function toggleThinkingExpanded(
  state: ConversationState,
  itemId: string,
): ConversationState {
  const items = state.items.map((it) => {
    if (it.id !== itemId || it.kind !== "thinking") return it;
    return { ...it, expanded: !it.expanded };
  });
  return { ...state, items };
}

export function setRunStatus(
  state: ConversationState,
  status: ConversationRunStatus,
): ConversationState {
  let next = { ...state, status };
  // Cancelling is only a local request state. ACP deltas already in transit
  // must keep appending to the same visible message until the backend emits
  // the terminal idle/error event.
  if (status === "idle" || status === "error") {
    next = freezeStreamingAssistant(next);
  }
  return next;
}

/** Fold consecutive read-only explore tools for display (UI layer helper). */
export function foldExploreTools(items: TimelineItem[]): TimelineItem[] {
  const out: TimelineItem[] = [];
  let batch: ToolItem[] = [];

  const flush = () => {
    if (batch.length === 0) return;
    if (batch.length === 1) {
      out.push(batch[0]);
    } else {
      const first = batch[0];
      const allDone = batch.every(
        (t) =>
          t.tool.phase === "completed" ||
          t.tool.phase === "failed" ||
          t.tool.phase === "cancelled",
      );
      out.push({
        ...first,
        id: `fold-${first.id}`,
        tool: {
          ...first.tool,
          title: `Explored ${batch.length} items`,
          kind: "explore_batch",
          phase: allDone ? "completed" : "running",
          result: {
            summary: batch.map((b) => b.tool.title).join(", ").slice(0, 200),
            redacted: false,
          },
        },
      });
    }
    batch = [];
  };

  for (const item of items) {
    if (
      item.kind === "tool" &&
      (item.tool.kind === "read" ||
        item.tool.kind === "search" ||
        item.tool.foldGroup === "explore")
    ) {
      batch.push(item);
    } else {
      flush();
      out.push(item);
    }
  }
  flush();
  return out;
}
