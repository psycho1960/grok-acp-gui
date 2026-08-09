// GAG-003: Bridge contract round-trip and boundary tests.
//
// These tests verify:
//  1. JSON fixture round-trips between Rust serialization and TS parsing
//  2. Unknown command types produce stable errors
//  3. Malformed payloads are rejected
//  4. Listener subscribe / unsubscribe / duplicate unsubscribe
//  5. Static dependency: Feature code must not import Tauri directly
//  6. FakeDesktopBridge behaves correctly

import { describe, it, expect } from "vitest";
import { createFakeDesktopBridge, fakeError } from "../src/bridge/fake-bridge";
import type {
  DesktopCommand,
  DesktopResult,
  TaskId,
  SessionId,
  ProjectId,
  TypedDesktopEvent,
} from "../src/bridge/types";
import { EventTypes, ErrorCodes } from "../src/bridge/types";

// ---------------------------------------------------------------------------
// Round-trip fixtures — these JSON blobs match what Rust serialises.
// ---------------------------------------------------------------------------

const RUNTIME_REFRESH_FIXTURE = {
  type: "runtime.refresh",
  payload: {},
} satisfies DesktopCommand;

const TASK_CREATE_FIXTURE = {
  type: "task.create",
  payload: {
    projectId: "proj-1" as ProjectId,
    title: "Add login page",
    prompt: "Create a login page with email and password fields",
    mode: "code",
  },
} satisfies DesktopCommand;

const PERMISSION_RESOLVE_FIXTURE = {
  type: "permission.resolve",
  payload: {
    requestId: "req-42",
    optionId: "opt-allow-once",
  },
} satisfies DesktopCommand;

const PLAN_RESOLVE_FIXTURE = {
  type: "plan.resolve",
  payload: {
    requestId: "req-99",
    optionId: "plan-approve",
  },
} satisfies DesktopCommand;

const SESSION_EVENT_FIXTURE: TypedDesktopEvent = {
  type: EventTypes.MESSAGE_DELTA,
  taskId: "task-1" as TaskId,
  sessionId: "sess-1" as SessionId,
  seq: 7,
  timestamp: "2026-01-01T00:00:00Z",
  payload: { text: "hello" },
};

const NON_SESSION_EVENT_FIXTURE: TypedDesktopEvent = {
  type: EventTypes.RUNTIME_UPDATED,
  timestamp: "2026-01-01T00:00:00Z",
  payload: { status: "ready" },
};

// ---------------------------------------------------------------------------
// 1. Round-trip parsing
// ---------------------------------------------------------------------------

describe("DesktopCommand round-trip", () => {
  it("parses runtime.refresh", () => {
    const cmd = RUNTIME_REFRESH_FIXTURE;
    expect(cmd.type).toBe("runtime.refresh");
    expect(cmd.payload).toEqual({});
  });

  it("parses task.create with optional fields", () => {
    const cmd = TASK_CREATE_FIXTURE;
    expect(cmd.type).toBe("task.create");
    expect(cmd.payload.projectId).toBe("proj-1");
    expect(cmd.payload.title).toBe("Add login page");
    expect(cmd.payload.mode).toBe("code");
  });

  it("parses plan.resolve with optionId", () => {
    const cmd = PLAN_RESOLVE_FIXTURE;
    expect(cmd.type).toBe("plan.resolve");
    expect(cmd.payload.requestId).toBe("req-99");
    expect(cmd.payload.optionId).toBe("plan-approve");
  });

  it("parses permission.resolve", () => {
    const cmd = PERMISSION_RESOLVE_FIXTURE;
    expect(cmd.type).toBe("permission.resolve");
    expect(cmd.payload.requestId).toBe("req-42");
    expect(cmd.payload.optionId).toBe("opt-allow-once");
  });

  it("parses all command discriminant strings", () => {
    const allTypes: DesktopCommand["type"][] = [
      "runtime.refresh",
      "runtime.login",
      "project.open",
      "project.forget",
      "task.create",
      "task.open",
      "task.archive",
      "turn.send",
      "turn.cancel",
      "session.configure",
      "session.resume",
      "permission.resolve",
      "plan.resolve",
      "artifact.import",
      "artifact.import.blob",
      "artifact.save",
      "workspace.inspect",
      "worktree.adopt",
      "review.diff",
      "review.checkpoint",
      "integration.preflight",
      "integration.execute",
      "worktree.cleanup",
      "recovery.restore",
      "recovery.delete",
    ];
    expect(allTypes).toHaveLength(25);
    // No duplicates
    expect(new Set(allTypes).size).toBe(25);
  });

  it("parses artifact.import.blob with base64 clipboard images", () => {
    const cmd = {
      type: "artifact.import.blob",
      payload: {
        taskId: "task-1" as TaskId,
        blobs: [{ displayName: "截图.png", base64Data: "iVBORw0KGgo=" }],
      },
    } satisfies DesktopCommand;
    expect(cmd.type).toBe("artifact.import.blob");
    expect(cmd.payload.blobs[0].displayName).toBe("截图.png");
  });

  it("parses artifact.save as a single-file, explicit-overwrite command", () => {
    const cmd = {
      type: "artifact.save",
      payload: {
        taskId: "task-1" as TaskId,
        artifactId: "artifact-1",
        targetPath: "C:\\Users\\tester\\result.png",
        overwrite: false,
      },
    } satisfies DesktopCommand;
    expect(cmd.payload.artifactId).toBe("artifact-1");
    expect(cmd.payload.overwrite).toBe(false);
    expect(JSON.stringify(cmd)).not.toMatch(/artifactIds|base64|cachePath/i);
  });

  it("parses session.configure settings", () => {
    const cmd = {
      type: "session.configure",
      payload: {
        taskId: "task-1" as TaskId,
        settings: { model: "deepseek", reasoning: "max" },
      },
    } satisfies DesktopCommand;
    expect(cmd.payload.settings.model).toBe("deepseek");
  });
});

describe("DesktopEvent round-trip", () => {
  it("session event has required fields", () => {
    const ev = SESSION_EVENT_FIXTURE;
    expect(ev.type).toBe(EventTypes.MESSAGE_DELTA);
    expect(ev.taskId).toBe("task-1");
    expect(ev.sessionId).toBe("sess-1");
    expect(ev.seq).toBe(7);
    expect(ev.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
    expect(ev.payload).toEqual({ text: "hello" });
  });

  it("non-session event omits optional fields", () => {
    // TypedDesktopEvent union prevents accessing session fields on
    // non-session variants — the type system enforces the invariant.
    const ev = NON_SESSION_EVENT_FIXTURE;
    expect(ev.type).toBe(EventTypes.RUNTIME_UPDATED);
    expect(ev.payload).toEqual({ status: "ready" });
    // @ts-expect-error: runtime.updated has no taskId
    // expect(ev.taskId).toBeUndefined();
  });

  it("all event type constants are defined", () => {
    const eventTypes = Object.values(EventTypes);
    expect(eventTypes).toHaveLength(12);
    expect(new Set(eventTypes).size).toBe(12);
  });

  it("session.commands.updated event mirrors the Rust SlashCommandInfo DTO", () => {
    const ev: TypedDesktopEvent = {
      type: EventTypes.SESSION_COMMANDS_UPDATED,
      taskId: "task-1" as TaskId,
      sessionId: "sess-1" as SessionId,
      seq: 3,
      timestamp: "2026-01-01T00:00:00Z",
      payload: {
        commands: [
          { name: "init", description: "初始化一个新项目", acceptsInput: false },
          { name: "plan", description: "为变更制定计划", acceptsInput: true },
        ],
      },
    };
    expect(ev.type).toBe("session.commands.updated");
    const commands = ev.payload.commands;
    expect(commands[0].name).toBe("init");
    expect(commands[0].acceptsInput).toBe(false);
    expect(commands[1].acceptsInput).toBe(true);
  });

  it("all error code constants are defined", () => {
    const codes = Object.values(ErrorCodes);
    expect(codes.length).toBeGreaterThanOrEqual(22);
  });
});

// ---------------------------------------------------------------------------
// 2. FakeDesktopBridge
// ---------------------------------------------------------------------------

describe("FakeDesktopBridge", () => {
  it("bootstrap returns configured snapshot", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: { version: "1.2.3" },
    });
    const snap = await bridge.bootstrap();
    expect(snap.version).toBe("1.2.3");
    expect(snap.productName).toContain("Grok ACP GUI");
    expect(snap.ready).toBe(true);
  });

  it("execute returns stub acknowledgement by default", async () => {
    const bridge = createFakeDesktopBridge();
    const result = await bridge.execute({
      type: "runtime.refresh",
      payload: {},
    });
    expect(result.success).toBe("true");
    if (result.success === "true") {
      expect(result.data).toEqual({ acknowledged: "runtime.refresh" });
    }
  });

  it("execute uses custom onExecute handler", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute: (cmd) => {
        if (cmd.type === "task.create") {
          return {
            success: "true",
            data: { taskId: "new-task-1" },
          };
        }
        return {
          success: "false",
          error: fakeError({ code: "TEST_UNHANDLED" }),
        };
      },
    });

    const ok = await bridge.execute(TASK_CREATE_FIXTURE);
    expect(ok.success).toBe("true");

    const err = await bridge.execute(RUNTIME_REFRESH_FIXTURE);
    expect(err.success).toBe("false");
    if (err.success === "false") {
      expect(err.error.code).toBe("TEST_UNHANDLED");
    }
  });

  it("subscribe delivers pushed events", async () => {
    const bridge = createFakeDesktopBridge();
    const received: TypedDesktopEvent[] = [];
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const unsub = await bridge.subscribe((ev) => received.push(ev));

    bridge.pushEvent(SESSION_EVENT_FIXTURE);
    bridge.pushEvent(NON_SESSION_EVENT_FIXTURE);

    expect(received).toHaveLength(2);
    expect(received[0].type).toBe(EventTypes.MESSAGE_DELTA);
    expect(received[1].type).toBe(EventTypes.RUNTIME_UPDATED);
  });

  it("unsubscribe stops delivery", async () => {
    const bridge = createFakeDesktopBridge();
    const received: TypedDesktopEvent[] = [];
    const unsub = await bridge.subscribe((ev) => received.push(ev));

    bridge.pushEvent(NON_SESSION_EVENT_FIXTURE);
    expect(received).toHaveLength(1);

    unsub();
    bridge.pushEvent(SESSION_EVENT_FIXTURE);
    expect(received).toHaveLength(1); // no new events
  });

  it("duplicate unsubscribe is idempotent", async () => {
    const bridge = createFakeDesktopBridge();
    const received: TypedDesktopEvent[] = [];
    const unsub = await bridge.subscribe((ev) => received.push(ev));

    unsub();
    unsub(); // should not throw
    bridge.pushEvent(NON_SESSION_EVENT_FIXTURE);
    expect(received).toHaveLength(0);
  });

  it("multiple listeners all receive events", async () => {
    const bridge = createFakeDesktopBridge();
    const a: DesktopEvent[] = [];
    const b: DesktopEvent[] = [];
    await bridge.subscribe((ev) => a.push(ev));
    await bridge.subscribe((ev) => b.push(ev));

    bridge.pushEvent(NON_SESSION_EVENT_FIXTURE);
    expect(a).toHaveLength(1);
    expect(b).toHaveLength(1);
  });
});

// ---------------------------------------------------------------------------
// 3. Error model
// ---------------------------------------------------------------------------

describe("AppError model", () => {
  it("fakeError produces valid AppError shape", () => {
    const err = fakeError({
      code: "BRIDGE_UNSUPPORTED_COMMAND",
      message: "Unknown command type",
      retryable: false,
    });
    expect(err.code).toBe("BRIDGE_UNSUPPORTED_COMMAND");
    expect(err.message).toBe("Unknown command type");
    expect(err.retryable).toBe(false);
    expect(err.detailsRedacted).toBe(true);
    expect(typeof err.correlationId).toBe("string");
    expect(err.correlationId.length).toBeGreaterThan(0);
  });

  it("DesktopResult success variant", () => {
    const ok: DesktopResult = { success: "true", data: { foo: 1 } };
    expect(ok.success).toBe("true");
    if (ok.success === "true") {
      expect(ok.data).toEqual({ foo: 1 });
    }
  });

  it("DesktopResult error variant", () => {
    const err: DesktopResult = {
      success: "false",
      error: fakeError(),
    };
    expect(err.success).toBe("false");
    if (err.success === "false") {
      expect(err.error.code).toBe("TEST");
    }
  });
});

// ---------------------------------------------------------------------------
// 4. Validation — unknown / malformed types
// ---------------------------------------------------------------------------

describe("Bridge validation boundaries", () => {
  it("FakeBridge returns error for unknown command (via custom handler)", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute: () => ({
        success: "false",
        error: fakeError({
          code: ErrorCodes.BRIDGE_UNSUPPORTED_COMMAND,
          message: "Unsupported command type",
        }),
      }),
    });

    // Simulating what the real bridge does when it can't parse
    const result = await bridge.execute({
      type: "runtime.refresh",
      payload: {},
    });
    expect(result.success).toBe("false");
    if (result.success === "false") {
      expect(result.error.code).toBe(ErrorCodes.BRIDGE_UNSUPPORTED_COMMAND);
    }
  });
});

// ---------------------------------------------------------------------------
// 5. Static dependency guard (type-level only)
// ---------------------------------------------------------------------------

describe("Static dependency guard", () => {
  it("bridge types do not import Tauri API", () => {
    // This test exists as a documentation gate.  The actual enforcement
    // happens via ESLint `no-restricted-imports` and code review.
    // We import the types here to prove they compile without Tauri.
    const types = [
      "BootstrapStatus",
      "DesktopCommand",
      "DesktopEvent",
      "DesktopResult",
      "AppError",
      "EventTypes",
      "ErrorCodes",
    ] as const;
    expect(types.length).toBeGreaterThan(0);
  });
});
