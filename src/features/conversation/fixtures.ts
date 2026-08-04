// GAG-008: Typed event fixtures for every known DesktopEvent kind + unknown.

import type {
  SessionId,
  TaskId,
  TypedDesktopEvent,
} from "../../bridge/types";
import type { SessionTimelineSnapshot } from "./types";

export const FIX_TASK = "task-conv-1" as TaskId;
export const FIX_SESSION = "sess-conv-1" as SessionId;

function ts(offsetSec = 0): string {
  return new Date(Date.UTC(2026, 3, 1, 12, 0, offsetSec)).toISOString();
}

function base(
  type: TypedDesktopEvent["type"],
  seq: number,
  payload: unknown,
  offsetSec = seq,
): TypedDesktopEvent {
  return {
    type,
    taskId: FIX_TASK,
    sessionId: FIX_SESSION,
    seq,
    timestamp: ts(offsetSec),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    payload: payload as any,
  } as TypedDesktopEvent;
}

/** User-facing message.delta text chunks. */
export function fixtureAssistantDelta(
  seq: number,
  text: string,
): TypedDesktopEvent {
  return base("message.delta", seq, { text });
}

export function fixtureToolDelta(
  seq: number,
  toolCall: Record<string, unknown>,
): TypedDesktopEvent {
  return base("message.delta", seq, { toolCall });
}

export function fixtureActivity(
  seq: number,
  kind: string,
  detail: string,
): TypedDesktopEvent {
  return base("activity.updated", seq, { kind, detail });
}

export function fixtureTaskState(
  seq: number,
  status: string,
  detail: unknown = null,
): TypedDesktopEvent {
  return base("task.state", seq, {
    taskId: FIX_TASK,
    status,
    detail,
  });
}

export function fixturePermission(
  seq: number,
  requestId = "perm-1",
): TypedDesktopEvent {
  return base("permission.requested", seq, {
    requestId,
    options: [
      { optionId: "allow-once-1", name: "Allow once", kind: "allow_once" },
      { optionId: "reject-1", name: "Reject", kind: "reject_once" },
    ],
    toolCall: {
      toolCallId: "tc-perm",
      title: "Run shell",
      kind: "execute",
      locations: ["src/app.ts"],
    },
  });
}

export function fixturePlan(seq: number, status = "awaiting_approval"): TypedDesktopEvent {
  return base("plan.updated", seq, {
    status,
    detail: { steps: ["Explore", "Edit", "Test"], version: 1 },
  });
}

export function fixtureArtifact(
  seq: number,
  artifactId = "art-1",
): TypedDesktopEvent {
  return base("artifact.available", seq, {
    taskId: FIX_TASK,
    artifactId,
    mimeType: "image/png",
    displayName: "screenshot.png",
  });
}

export function fixtureChanges(seq: number): TypedDesktopEvent {
  return base("changes.updated", seq, {
    taskId: FIX_TASK,
    files: [{ path: "a.ts", status: "modified" }],
  });
}

export function fixtureDiagnostic(
  level = "info",
  message = "runtime notice",
): TypedDesktopEvent {
  return {
    type: "diagnostic.notice",
    timestamp: ts(0),
    payload: { level, message, source: "runtime" },
  };
}

export function fixtureResourceWarning(
  message = "4+ concurrent turns",
): TypedDesktopEvent {
  return {
    type: "resource.warning",
    timestamp: ts(0),
    payload: { message, resource: "turns" },
  };
}

/** Unknown event type — must render as safe fallback. */
export function fixtureUnknown(
  seq: number,
  eventType = "future.widget",
): TypedDesktopEvent {
  return {
    type: eventType,
    taskId: FIX_TASK,
    sessionId: FIX_SESSION,
    seq,
    timestamp: ts(seq),
    payload: {
      secret: "SHOULD_NOT_RENDER_AS_JSON",
      nested: { token: "x" },
    },
  } as TypedDesktopEvent;
}

export function fixtureToolLifecycle(): TypedDesktopEvent[] {
  return [
    fixtureToolDelta(10, {
      toolCallId: "tc-1",
      title: "Read file",
      kind: "read",
      status: "running",
      inputSummary: "src/main.ts",
      inputRedacted: false,
      startedAt: ts(10),
    }),
    fixtureToolDelta(11, {
      toolCallId: "tc-1",
      title: "Read file",
      kind: "read",
      status: "completed",
      resultSummary: "42 lines",
      resultRedacted: false,
      endedAt: ts(12),
      durationMs: 2000,
    }),
    fixtureToolDelta(12, {
      toolCallId: "tc-2",
      title: "Run tests",
      kind: "execute",
      status: "running",
      inputSummary: "npm test",
      inputRedacted: false,
      startedAt: ts(12),
    }),
    fixtureToolDelta(13, {
      toolCallId: "tc-2",
      status: "failed",
      resultSummary: "exit 1",
      exitCode: 1,
      endedAt: ts(15),
      durationMs: 3000,
    }),
  ];
}

/** Redacted tool input/result — frontend must not invent content. */
export function fixtureRedactedTool(seq: number): TypedDesktopEvent {
  return fixtureToolDelta(seq, {
    toolCallId: "tc-secret",
    title: "Shell",
    kind: "execute",
    status: "completed",
    redacted: true,
    inputSummary: "[redacted]",
    resultSummary: "[redacted]",
  });
}

/** Full sample conversation as ordered events. */
export function fixtureConversationEvents(): TypedDesktopEvent[] {
  return [
    fixtureTaskState(1, "running"),
    fixtureActivity(2, "thinking", "Planning approach"),
    fixtureAssistantDelta(3, "I'll "),
    fixtureAssistantDelta(4, "inspect the "),
    fixtureAssistantDelta(5, "module."),
    ...fixtureToolLifecycle(),
    fixturePermission(20),
    fixturePlan(21),
    fixtureArtifact(22),
    fixtureChanges(23),
    fixtureTaskState(24, "waiting_permission"),
    fixtureUnknown(25),
    fixtureRedactedTool(26),
  ];
}

export function fixtureSessionSnapshot(
  overrides: Partial<SessionTimelineSnapshot> = {},
): SessionTimelineSnapshot {
  return {
    taskId: FIX_TASK,
    sessionId: FIX_SESSION,
    title: "Conversation fixture",
    status: "running",
    cursor: 5,
    events: [
      fixtureTaskState(1, "running"),
      fixtureAssistantDelta(2, "Hello from snapshot. "),
      fixtureAssistantDelta(3, "History frozen."),
      fixtureToolDelta(4, {
        toolCallId: "tc-snap",
        title: "List dir",
        kind: "read",
        status: "completed",
        inputSummary: "src/",
        resultSummary: "12 files",
        durationMs: 15,
      }),
      fixtureActivity(5, "status", "Snapshot complete"),
    ],
    attempt: 1,
    ...overrides,
  };
}

/** 10k-event generator for virtualization / perf tests. */
export function generateManyEvents(count: number): TypedDesktopEvent[] {
  const events: TypedDesktopEvent[] = [];
  for (let i = 1; i <= count; i++) {
    if (i % 7 === 0) {
      events.push(
        fixtureToolDelta(i, {
          toolCallId: `tc-bulk-${Math.floor(i / 7)}`,
          title: `Tool ${i}`,
          kind: i % 14 === 0 ? "execute" : "read",
          status: i % 14 === 0 ? "completed" : "running",
          inputSummary: `arg-${i}`,
        }),
      );
    } else if (i % 11 === 0) {
      events.push(fixtureActivity(i, "heartbeat", `tick ${i}`));
    } else {
      events.push(fixtureAssistantDelta(i, `word${i} `));
    }
  }
  return events;
}
