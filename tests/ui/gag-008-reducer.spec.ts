import { describe, expect, it } from "vitest";
import {
  fixtureAssistantDelta,
  fixtureConversationEvents,
  fixturePermission,
  fixtureSessionSnapshot,
  fixtureTaskState,
  fixtureToolDelta,
  fixtureUnknown,
  FIX_SESSION,
  FIX_TASK,
  generateManyEvents,
} from "../../src/features/conversation/fixtures";
import {
  applyEvent,
  applyEvents,
  applySnapshot,
  appendUserMessage,
  createEmptyConversationState,
  foldExploreTools,
  setRunStatus,
} from "../../src/features/conversation/reducer";
import type { SessionId, TaskId, TypedDesktopEvent } from "../../src/bridge/types";

function fixtureUserDelta(seq: number, text: string): TypedDesktopEvent {
  return {
    type: "message.delta",
    taskId: FIX_TASK,
    sessionId: FIX_SESSION,
    seq,
    timestamp: `2026-08-05T12:00:0${seq}.000Z`,
    payload: { role: "user", text },
  };
}

describe("GAG-008 conversation reducer", () => {
  it("ignores a live user chunk that was not sent by the local composer", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = appendUserMessage(state, "检查项目", {
      id: "local-user",
      pending: true,
    });
    state = applyEvent(state, fixtureUserDelta(1, "检查项目"), {
      acceptUnmatchedUserMessages: false,
    });
    state = applyEvent(
      state,
      fixtureUserDelta(2, "探索子目录并向父 Agent 报告，不要修改文件"),
      { acceptUnmatchedUserMessages: false },
    );

    const users = state.items.filter((item) => item.kind === "user");
    expect(users).toHaveLength(1);
    expect(users[0]?.kind === "user" && users[0].text).toBe("检查项目");
  });

  it("keeps user chunks when rebuilding an authoritative history snapshot", () => {
    const snapshot = fixtureSessionSnapshot({
      cursor: 1,
      events: [fixtureUserDelta(1, "历史用户消息")],
    });
    const state = applySnapshot(createEmptyConversationState(FIX_TASK), snapshot);

    expect(state.items.filter((item) => item.kind === "user")).toHaveLength(1);
  });

  it("dedupes by sessionId+seq", () => {
    let state = createEmptyConversationState(FIX_TASK);
    const e = fixtureAssistantDelta(1, "Hi");
    state = applyEvent(state, e);
    state = applyEvent(state, e);
    expect(state.items.filter((i) => i.kind === "assistant")).toHaveLength(1);
    expect(state.cursor.lastSeq).toBe(1);
  });

  it("keeps the input state immutable while reusing private batch containers", () => {
    const input = createEmptyConversationState(FIX_TASK);
    const inputItems = input.items;
    const inputSeenKeys = input.seenKeys;
    const inputToolIndex = input.toolIndex;
    const inputPendingEvents = input.pendingEvents;

    const state = applyEvents(input, [
      fixtureAssistantDelta(1, "A"),
      fixtureToolDelta(2, {
        toolCallId: "batch-tool",
        title: "读取文件",
        kind: "read",
        status: "completed",
      }),
      fixtureAssistantDelta(3, "B"),
    ]);

    expect(input.items).toBe(inputItems);
    expect(input.items).toHaveLength(0);
    expect(input.seenKeys).toBe(inputSeenKeys);
    expect(input.seenKeys.size).toBe(0);
    expect(input.toolIndex).toBe(inputToolIndex);
    expect(input.toolIndex.size).toBe(0);
    expect(input.pendingEvents).toBe(inputPendingEvents);
    expect(input.pendingEvents.size).toBe(0);
    expect(state.items).not.toBe(inputItems);
    expect(state.seenKeys).not.toBe(inputSeenKeys);
    expect(state.toolIndex).not.toBe(inputToolIndex);
    expect(state.pendingEvents).not.toBe(inputPendingEvents);
    expect(state.cursor.lastSeq).toBe(3);
  });

  it("does not create a blank assistant message from whitespace-only deltas", () => {
    const state = applyEvents(createEmptyConversationState(FIX_TASK), [
      fixtureAssistantDelta(1, "\n\n"),
    ]);

    expect(state.items.filter((item) => item.kind === "assistant")).toHaveLength(0);
    expect(state.cursor.lastSeq).toBe(1);
  });

  it("keeps whitespace chunks inside a visible assistant message", () => {
    const state = applyEvents(createEmptyConversationState(FIX_TASK), [
      fixtureAssistantDelta(1, "Hello"),
      fixtureAssistantDelta(2, " "),
      fixtureAssistantDelta(3, "world"),
    ]);

    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind === "assistant" && assistant.text).toBe("Hello world");
  });

  it("does not replace streamed text with a whitespace-only completion", () => {
    const state = applyEvents(createEmptyConversationState(FIX_TASK), [
      fixtureAssistantDelta(1, "已有正文"),
      {
        type: "message.delta",
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        seq: 2,
        timestamp: "2026-08-05T12:00:02.000Z",
        payload: { completed: true, fullText: "\n\n" },
      },
    ]);

    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind === "assistant" && assistant.text).toBe("已有正文");
    expect(assistant?.kind === "assistant" && assistant.frozen).toBe(true);
  });

  it("appends assistant deltas into one streaming message then freezes on task idle", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureTaskState(1, "running"),
      fixtureAssistantDelta(2, "A"),
      fixtureAssistantDelta(3, "B"),
      fixtureAssistantDelta(4, "C"),
      fixtureTaskState(5, "merged"),
    ]);
    const assistants = state.items.filter((i) => i.kind === "assistant");
    expect(assistants).toHaveLength(1);
    if (assistants[0].kind !== "assistant") throw new Error("expected assistant");
    expect(assistants[0].text).toBe("ABC");
    expect(assistants[0].streaming).toBe(false);
    expect(assistants[0].frozen).toBe(true);
    expect(state.status).toBe("idle");
  });

  it("finishes a running turn from the live assistant-completed message", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureTaskState(1, "running"),
      fixtureAssistantDelta(2, "最终答复"),
      {
        type: "message.delta",
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        seq: 3,
        timestamp: "2026-08-05T12:00:03.000Z",
        payload: { completed: true, fullText: "最终答复" },
      },
    ]);

    expect(state.status).toBe("idle");
    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind === "assistant" && assistant.text).toBe("最终答复");
    expect(assistant?.kind === "assistant" && assistant.streaming).toBe(false);
    expect(assistant?.kind === "assistant" && assistant.frozen).toBe(true);
  });

  it("keeps late buffered deltas in one assistant message while cancellation is pending", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureTaskState(1, "running"),
      fixtureAssistantDelta(2, "partial"),
    ]);

    state = setRunStatus(state, "cancelling");
    state = applyEvent(state, fixtureAssistantDelta(3, " tail"));
    state = applyEvent(
      state,
      fixtureTaskState(4, "idle", { reason: "cancelled" }),
    );

    const assistants = state.items.filter((item) => item.kind === "assistant");
    expect(assistants).toHaveLength(1);
    expect(assistants[0]?.kind === "assistant" && assistants[0].text).toBe(
      "partial tail",
    );
    expect(
      assistants[0]?.kind === "assistant" && assistants[0].frozen,
    ).toBe(true);
  });

  it("merges tool lifecycle by toolCallId and rejects completed→running", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureToolDelta(1, {
        toolCallId: "tc-x",
        title: "Shell",
        kind: "execute",
        status: "running",
        inputSummary: "echo hi",
        startedAt: "2026-04-01T12:00:00.000Z",
      }),
      fixtureToolDelta(2, {
        toolCallId: "tc-x",
        status: "completed",
        resultSummary: "ok",
        endedAt: "2026-04-01T12:00:01.000Z",
        durationMs: 1000,
      }),
      // Late / out-of-order downgrade attempt
      fixtureToolDelta(3, {
        toolCallId: "tc-x",
        status: "running",
      }),
    ]);
    const tools = state.items.filter((i) => i.kind === "tool");
    expect(tools).toHaveLength(1);
    if (tools[0].kind !== "tool") throw new Error("tool");
    expect(tools[0].tool.phase).toBe("completed");
    expect(tools[0].tool.result.summary).toBe("ok");
    expect(tools[0].tool.durationMs).toBe(1000);
  });

  it("never retains a tool summary whose backend field is marked redacted", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvent(
      state,
      fixtureToolDelta(1, {
        toolCallId: "tc-defence",
        title: "Shell",
        kind: "execute",
        status: "completed",
        inputSummary: "API_KEY=must-never-render",
        inputRedacted: true,
        resultSummary: "TOKEN=must-never-copy",
        resultRedacted: true,
      }),
    );

    const serialized = JSON.stringify(state.items);
    expect(serialized).not.toContain("must-never-render");
    expect(serialized).not.toContain("must-never-copy");
    expect(serialized).toContain("[redacted]");
  });

  it("leaves running state when the active ACP session process exits", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureTaskState(1, "running"),
      fixtureAssistantDelta(2, "partial response"),
    ]);

    state = applyEvent(state, {
      type: "runtime.updated",
      timestamp: "2026-08-05T00:00:03.000Z",
      payload: {
        status: "exited",
        sessionId: FIX_SESSION,
        reason: "session disconnected",
      },
    });

    expect(state.status).toBe("disconnected");
    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind === "assistant" && assistant.frozen).toBe(true);
  });

  it("turns a terminal request failure into a coded recoverable error state", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureTaskState(1, "running"),
      fixtureAssistantDelta(2, "partial response"),
      {
        type: "activity.updated",
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        seq: 3,
        timestamp: "2026-08-05T00:00:03.000Z",
        payload: {
          kind: "error",
          detail: "Grok Build usage balance exhausted",
          code: "GROK_USAGE_EXHAUSTED",
          retryable: true,
        },
      } as TypedDesktopEvent,
    ]);

    expect(state.status).toBe("error");
    const error = state.items.find((item) => item.kind === "error");
    expect(error?.kind === "error" && error.code).toBe("GROK_USAGE_EXHAUSTED");
    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind === "assistant" && assistant.frozen).toBe(true);
  });

  it("ignores events for other tasks", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvent(state, fixtureAssistantDelta(1, "mine"));
    state = applyEvent(state, {
      type: "message.delta",
      taskId: "other" as TaskId,
      sessionId: "s-other" as SessionId,
      seq: 2,
      timestamp: "2026-04-01T12:00:02.000Z",
      payload: { text: "theirs" },
    });
    expect(state.items).toHaveLength(1);
  });

  it("renders unknown events as safe fallback without raw JSON", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvent(state, fixtureUnknown(1, "future.widget"));
    expect(state.items).toHaveLength(1);
    const u = state.items[0];
    if (u.kind !== "unknown") throw new Error("unknown");
    expect(u.eventType).toBe("future.widget");
    expect(u.safeSummary).toContain("future.widget");
    expect(JSON.stringify(u)).not.toContain("SHOULD_NOT_RENDER");
  });

  it("applies snapshot cursor so historical seqs are not re-applied", () => {
    let state = createEmptyConversationState(FIX_TASK);
    const snap = fixtureSessionSnapshot();
    state = applySnapshot(state, snap);
    expect(state.cursor.snapshotSeq).toBe(5);
    expect(state.cursor.lastSeq).toBeGreaterThanOrEqual(5);
    const before = state.items.length;
    // Re-send seq 2 delta — must ignore
    state = applyEvent(state, fixtureAssistantDelta(2, "REPLAY_SHOULD_IGNORE"));
    expect(state.items.length).toBe(before);
    expect(JSON.stringify(state.items)).not.toContain("REPLAY_SHOULD_IGNORE");
    // New seq after cursor applies
    state = applyEvent(state, fixtureAssistantDelta(6, " after"));
    const text = state.items
      .filter((i) => i.kind === "assistant")
      .map((i) => (i.kind === "assistant" ? i.text : ""))
      .join("");
    expect(text).toContain("after");
  });

  it("rebuilds an authoritative compact snapshot whose event sequences are sparse", () => {
    const user = {
      ...fixtureAssistantDelta(10, "historical question"),
      payload: { role: "user" as const, text: "historical question" },
    };
    const assistant = fixtureAssistantDelta(590, "historical answer");
    const interrupted = fixtureTaskState(600, "interrupted");

    const state = applySnapshot(createEmptyConversationState(FIX_TASK), {
      ...fixtureSessionSnapshot(),
      cursor: 600,
      events: [user, assistant, interrupted],
      status: "error",
    });

    expect(state.items.some((item) => item.kind === "user")).toBe(true);
    expect(
      state.items.some(
        (item) => item.kind === "assistant" && item.text === "historical answer",
      ),
    ).toBe(true);
    expect(
      state.items.some(
        (item) => item.kind === "system" && item.message === "任务已中断",
      ),
    ).toBe(true);
    expect(state.status).toBe("error");
    expect(state.cursor).toEqual({ lastSeq: 600, snapshotSeq: 600 });
    expect(state.needsSnapshotRefresh).toBe(false);
  });

  it("detects sequence gaps after snapshot", () => {
    let state = applySnapshot(
      createEmptyConversationState(FIX_TASK),
      fixtureSessionSnapshot({ cursor: 5 }),
    );
    state = applyEvent(state, fixtureAssistantDelta(9, "jump"));
    expect(state.needsSnapshotRefresh).toBe(true);
    expect(state.gapFromSeq).toBe(6);
  });

  it("buffers out-of-order events and drains them strictly by sequence", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvent(state, fixtureAssistantDelta(1, "A"));
    state = applyEvent(state, fixtureAssistantDelta(3, "C"));

    const beforeGap = state.items.find((item) => item.kind === "assistant");
    expect(beforeGap?.kind === "assistant" && beforeGap.text).toBe("A");
    expect(state.needsSnapshotRefresh).toBe(true);
    expect(state.gapFromSeq).toBe(2);

    state = applyEvent(state, fixtureAssistantDelta(2, "B"));
    const recovered = state.items.find((item) => item.kind === "assistant");
    expect(recovered?.kind === "assistant" && recovered.text).toBe("ABC");
    expect(state.cursor.lastSeq).toBe(3);
    expect(state.needsSnapshotRefresh).toBe(false);
    expect(state.gapFromSeq).toBeNull();
  });

  it("creates permission and plan slots", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixturePermission(1),
      {
        type: "plan.updated",
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        seq: 2,
        timestamp: "2026-04-01T12:00:02.000Z",
        payload: { status: "awaiting_approval", detail: { steps: [1] } },
      },
    ]);
    expect(state.items.some((i) => i.kind === "permission")).toBe(true);
    expect(state.items.some((i) => i.kind === "plan")).toBe(true);
    expect(state.status).toBe("waiting_plan");
  });

  it("handles full fixture conversation without throwing", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, fixtureConversationEvents());
    expect(state.items.length).toBeGreaterThan(5);
    expect(state.seenKeys.size).toBeGreaterThan(5);
    const kinds = new Set(state.items.map((i) => i.kind));
    expect(kinds.has("assistant")).toBe(true);
    expect(kinds.has("tool")).toBe(true);
    expect(kinds.has("permission")).toBe(true);
    expect(kinds.has("unknown")).toBe(true);
  });

  it("appends optimistic user messages", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = {
      ...state,
      sessionId: FIX_SESSION,
    };
    state = appendUserMessage(state, "Hello agent", { id: "u1", pending: true });
    expect(state.items[0].kind).toBe("user");
    if (state.items[0].kind === "user") {
      expect(state.items[0].text).toBe("Hello agent");
      expect(state.items[0].pending).toBe(true);
    }
  });

  it("reconciles the confirmed ACP user echo without duplicating the optimistic message", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = { ...state, sessionId: FIX_SESSION };
    state = appendUserMessage(state, "Hello once", { id: "local-user", pending: true });
    state = applyEvent(state, {
      type: "message.delta",
      taskId: FIX_TASK,
      sessionId: FIX_SESSION,
      seq: 1,
      timestamp: "2026-08-05T00:00:00.000Z",
      payload: { role: "user", text: "Hello once" },
    });

    const users = state.items.filter((item) => item.kind === "user");
    expect(users).toHaveLength(1);
    expect(users[0].kind === "user" && users[0].pending).toBe(false);
  });

  it("restores a confirmed user event and makes cancellation explicit", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      {
        type: "message.delta",
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        seq: 1,
        timestamp: "2026-08-05T00:00:00.000Z",
        payload: { role: "user", text: "restored user" },
      },
      fixtureAssistantDelta(2, "partial"),
      {
        type: "task.state",
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        seq: 3,
        timestamp: "2026-08-05T00:00:02.000Z",
        payload: {
          taskId: FIX_TASK,
          status: "idle",
          detail: { reason: "cancelled" },
        },
      },
    ]);

    expect(state.items.filter((item) => item.kind === "user")).toHaveLength(1);
    expect(state.items.some((item) => item.kind === "system" && item.message === "已停止")).toBe(true);
    const assistant = state.items.find((item) => item.kind === "assistant");
    expect(assistant?.kind === "assistant" && assistant.frozen).toBe(true);
    expect(state.status).toBe("idle");
  });

  it("folds consecutive read tools into Explored N items", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureToolDelta(1, {
        toolCallId: "a",
        title: "A",
        kind: "read",
        status: "completed",
      }),
      fixtureToolDelta(2, {
        toolCallId: "b",
        title: "B",
        kind: "read",
        status: "completed",
      }),
      fixtureToolDelta(3, {
        toolCallId: "c",
        title: "C",
        kind: "execute",
        status: "completed",
      }),
    ]);
    const folded = foldExploreTools(state.items);
    expect(folded[0].kind).toBe("tool");
    if (folded[0].kind === "tool") {
      expect(folded[0].tool.title).toBe("已查看 2 项");
    }
    expect(folded[1].kind).toBe("tool");
    if (folded[1].kind === "tool") {
      expect(folded[1].tool.kind).toBe("execute");
    }
  });

  it("folds consecutive Grok Build read-only tool titles into one explore batch", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, [
      fixtureToolDelta(1, {
        toolCallId: "a",
        title: "list_dir",
        kind: "other",
        status: "completed",
      }),
      fixtureToolDelta(2, {
        toolCallId: "b",
        title: "grep",
        kind: "other",
        status: "completed",
      }),
      fixtureToolDelta(3, {
        toolCallId: "c",
        title: "search_replace",
        kind: "edit",
        status: "completed",
      }),
    ]);
    const folded = foldExploreTools(state.items);
    expect(folded).toHaveLength(2);
    if (folded[0].kind === "tool") {
      expect(folded[0].tool.title).toBe("已查看 2 项");
    }
    if (folded[1].kind === "tool") {
      expect(folded[1].tool.title).toBe("search_replace");
    }
  });

  it("processes 10k events within a reasonable budget", () => {
    const events = generateManyEvents(10_000);
    const start = performance.now();
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, events);
    const ms = performance.now() - start;
    expect(state.cursor.lastSeq).toBe(10_000);
    expect(state.items.length).toBeGreaterThan(100);
    // Generous CI budget — correctness first
    expect(ms).toBeLessThan(5000);
  });
});
