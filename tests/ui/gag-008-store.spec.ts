import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createFakeDesktopBridge, fakeError } from "../../src/bridge/fake-bridge";
import type { TaskId } from "../../src/bridge/types";
import { useConversationStore } from "../../src/features/conversation/conversation-store";
import {
  FIX_TASK,
  fixtureAssistantDelta,
  fixtureSessionSnapshot,
  fixtureTaskState,
} from "../../src/features/conversation/fixtures";

describe("GAG-008 conversation store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
    sessionStorage.clear();
  });

  it("loads snapshot and merges live deltas", async () => {
    const bridge = createFakeDesktopBridge();
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot());
    expect(store.loadState).toBe("ready");
    expect(store.cursor.snapshotSeq).toBe(5);

    store.injectEventForTest(fixtureAssistantDelta(6, " live"));
    store.flushForTest();
    const text = store.items
      .filter((i) => i.kind === "assistant")
      .map((i) => (i.kind === "assistant" ? i.text : ""))
      .join("");
    expect(text).toContain("live");
  });

  it("sends message, clears draft on success, keeps draft on failure", async () => {
    let fail = false;
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "turn.send") {
          if (fail) {
            return {
              success: "false",
              error: fakeError({ message: "network", retryable: true }),
            };
          }
          return { success: "true", data: { seq: 1 } };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ status: "idle", cursor: 0, events: [] }),
    );
    // Force idle for send
    store.injectEventForTest(fixtureTaskState(1, "merged"));

    store.setDraft("hello world");
    const ok = await store.sendMessage();
    expect(ok).toBe(true);
    expect(store.draft).toBe("");
    expect(store.items.some((i) => i.kind === "user")).toBe(true);

    store.setDraft("retry me");
    fail = true;
    // Need idle again
    store.injectEventForTest(fixtureTaskState(2, "merged"));
    const bad = await store.sendMessage();
    expect(bad).toBe(false);
    expect(store.draft).toBe("retry me");
    expect(store.sendError).toContain("network");
  });

  it("updates a provisional task title from the first successful turn", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "turn.send") {
          return {
            success: "true",
            data: { requestId: 1, taskTitle: "修复登录页面" },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ title: "新任务", status: "idle", cursor: 0, events: [] }),
    );
    store.setDraft("修复登录页面。并补充回归测试。");

    expect(await store.sendMessage()).toBe(true);
    expect(store.title).toBe("修复登录页面");
  });

  it("disables send when offline and preserves draft", async () => {
    const bridge = createFakeDesktopBridge();
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle" }));
    store.setDraft("keep me");
    store.injectEventForTest({
      type: "runtime.updated",
      timestamp: new Date().toISOString(),
      payload: { status: "unavailable" },
    });
    expect(store.composerCapabilities.canSend).toBe(false);
    expect(store.composerCapabilities.disabledReason).toMatch(/离线/);
    expect(store.draft).toBe("keep me");
  });

  it("batches high-frequency text deltas without reordering", async () => {
    vi.useFakeTimers();
    const bridge = createFakeDesktopBridge();
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ cursor: 0, events: [], status: "running" }),
    );
    for (let i = 1; i <= 20; i++) {
      store.injectEventForTest(fixtureAssistantDelta(i, `${i}`));
    }
    vi.advanceTimersByTime(50);
    store.flushForTest();
    const assistant = store.items.find((i) => i.kind === "assistant");
    expect(assistant && assistant.kind === "assistant" && assistant.text).toBe(
      "1234567891011121314151617181920",
    );
    vi.useRealTimers();
  });

  it("cancels turn via facade", async () => {
    const calls: string[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        calls.push(command.type);
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "running" }));
    store.injectEventForTest(fixtureTaskState(10, "running"));
    const ok = await store.cancelTurn();
    expect(ok).toBe(true);
    expect(calls).toContain("turn.cancel");
  });

  it("persists draft per task id", async () => {
    const bridge = createFakeDesktopBridge();
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ taskId: FIX_TASK }));
    store.setDraft("draft-a");
    expect(localStorage.getItem(`gag008:draft:${FIX_TASK}`)).toBe("draft-a");

    const store2 = useConversationStore();
    store2.openFromSnapshot(fixtureSessionSnapshot({ taskId: FIX_TASK }));
    expect(store2.draft).toBe("draft-a");
  });

  it("openTask without snapshot still sets task id", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "task.open") {
          return {
            success: "true",
            data: { taskId: command.payload.taskId, title: "T", status: "idle" },
          };
        }
        return { success: "true", data: {} };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    await store.openTask("task-x" as TaskId, "X");
    expect(store.taskId).toBe("task-x");
    expect(store.title).toBe("T");
  });

  it("opens an empty task as an idle conversation with a provisional title", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "task.open") {
          return {
            success: "true",
            data: { taskId: command.payload.taskId, title: "", status: "idle" },
          };
        }
        return { success: "true", data: {} };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);

    await store.openTask("task-empty" as TaskId);

    expect(store.title).toBe("新任务");
    expect(store.status).toBe("idle");
  });

  it("applies the persisted task.open timeline instead of showing an empty demo", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type !== "task.open") {
          return { success: "true", data: {} };
        }
        return {
          success: "true",
          data: {
            taskId: command.payload.taskId,
            sessionId: "session-restored",
            title: "Restored",
            status: "idle",
            attempt: 2,
            cursor: { lastSeq: 3, snapshotSeq: 3 },
            events: [
              {
                type: "message.delta",
                taskId: command.payload.taskId,
                sessionId: "session-restored",
                seq: 1,
                timestamp: "2026-08-05T00:00:00.000Z",
                payload: { role: "user", text: "persist me" },
              },
              {
                type: "message.delta",
                taskId: command.payload.taskId,
                sessionId: "session-restored",
                seq: 2,
                timestamp: "2026-08-05T00:00:01.000Z",
                payload: { role: "assistant", text: "restored reply" },
              },
              {
                type: "task.state",
                taskId: command.payload.taskId,
                sessionId: "session-restored",
                seq: 3,
                timestamp: "2026-08-05T00:00:02.000Z",
                payload: { taskId: command.payload.taskId, status: "idle", detail: {} },
              },
            ],
          },
        };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    await store.openTask("task-restored" as TaskId);

    expect(store.sessionId).toBe("session-restored");
    expect(store.attempt).toBe(2);
    expect(store.items.filter((item) => item.kind === "user")).toHaveLength(1);
    expect(store.items.filter((item) => item.kind === "assistant")).toHaveLength(1);
    expect(JSON.stringify(store.items)).toContain("restored reply");
  });

  it("resumes an interrupted session through DesktopBridge", async () => {
    const calls: string[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        calls.push(command.type);
        if (command.type === "session.resume") {
          return { success: "true", data: { sessionId: "resumed-session" } };
        }
        if (command.type === "task.open") {
          return {
            success: "true",
            data: { taskId: command.payload.taskId, title: "Resumed", status: "idle" },
          };
        }
        return { success: "true", data: {} };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle" }));
    store.injectEventForTest(fixtureTaskState(40, "interrupted"));

    expect(await store.resumeSession()).toBe(true);
    expect(calls).toContain("session.resume");
    expect(store.status).toBe("idle");
  });

  it("switches title, state, timeline, and draft atomically between tasks", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type !== "task.open") {
          return { success: "true", data: {} };
        }
        const isA = command.payload.taskId === "task-a";
        const taskId = command.payload.taskId;
        const sessionId = isA ? "session-a" : "session-b";
        return {
          success: "true",
          data: {
            taskId,
            sessionId,
            title: isA ? "Title A" : "Title B",
            status: isA ? "idle" : "running",
            attempt: 1,
            cursor: { lastSeq: 1, snapshotSeq: 1 },
            events: [
              {
                type: "message.delta",
                taskId,
                sessionId,
                seq: 1,
                timestamp: "2026-08-05T00:00:00.000Z",
                payload: { role: "user", text: isA ? "only A" : "only B" },
              },
            ],
          },
        };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);

    await store.openTask("task-a" as TaskId);
    store.setDraft("draft A");
    expect(store.title).toBe("Title A");
    expect(JSON.stringify(store.items)).toContain("only A");

    await store.openTask("task-b" as TaskId);
    expect(store.title).toBe("Title B");
    expect(store.status).toBe("running");
    expect(JSON.stringify(store.items)).toContain("only B");
    expect(JSON.stringify(store.items)).not.toContain("only A");
    expect(store.draft).toBe("");

    await store.openTask("task-a" as TaskId);
    expect(store.title).toBe("Title A");
    expect(store.status).toBe("idle");
    expect(JSON.stringify(store.items)).not.toContain("only B");
    expect(store.draft).toBe("draft A");
  });

  it("never lets a stale task.open response overwrite the latest selected task", async () => {
    let releaseA: (() => void) | null = null;
    const delayedA = new Promise<void>((resolve) => {
      releaseA = resolve;
    });
    const bridge = createFakeDesktopBridge({
      async onExecute(command) {
        if (command.type !== "task.open") {
          return { success: "true", data: {} };
        }
        if (command.payload.taskId === "task-a") {
          await delayedA;
        }
        const taskId = command.payload.taskId;
        const suffix = taskId === "task-a" ? "A" : "B";
        return {
          success: "true",
          data: {
            taskId,
            sessionId: `session-${suffix.toLowerCase()}`,
            title: `Title ${suffix}`,
            status: "idle",
            attempt: 1,
            cursor: { lastSeq: 1, snapshotSeq: 1 },
            events: [
              {
                type: "message.delta",
                taskId,
                sessionId: `session-${suffix.toLowerCase()}`,
                seq: 1,
                timestamp: "2026-08-05T00:00:00.000Z",
                payload: { role: "user", text: `only ${suffix}` },
              },
            ],
          },
        };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);

    const openingA = store.openTask("task-a" as TaskId);
    const openingB = store.openTask("task-b" as TaskId);
    await openingB;
    releaseA?.();
    await openingA;

    expect(store.taskId).toBe("task-b");
    expect(store.title).toBe("Title B");
    expect(JSON.stringify(store.items)).toContain("only B");
    expect(JSON.stringify(store.items)).not.toContain("only A");
  });
});
