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
    store.injectEventForTest(fixtureTaskState(50, "merged"));
    const bad = await store.sendMessage();
    expect(bad).toBe(false);
    expect(store.draft).toBe("retry me");
    expect(store.sendError).toContain("network");
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
    expect(sessionStorage.getItem(`gag008:draft:${FIX_TASK}`)).toBe("draft-a");

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
});
