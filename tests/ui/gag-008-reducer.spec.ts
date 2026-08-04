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
} from "../../src/features/conversation/reducer";
import type { SessionId, TaskId } from "../../src/bridge/types";

describe("GAG-008 conversation reducer", () => {
  it("dedupes by sessionId+seq", () => {
    let state = createEmptyConversationState(FIX_TASK);
    const e = fixtureAssistantDelta(1, "Hi");
    state = applyEvent(state, e);
    state = applyEvent(state, e);
    expect(state.items.filter((i) => i.kind === "assistant")).toHaveLength(1);
    expect(state.cursor.lastSeq).toBe(1);
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

  it("detects sequence gaps after snapshot", () => {
    let state = applySnapshot(
      createEmptyConversationState(FIX_TASK),
      fixtureSessionSnapshot({ cursor: 5 }),
    );
    state = applyEvent(state, fixtureAssistantDelta(9, "jump"));
    expect(state.needsSnapshotRefresh).toBe(true);
    expect(state.gapFromSeq).toBe(6);
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
      expect(folded[0].tool.title).toBe("Explored 2 items");
    }
    expect(folded[1].kind).toBe("tool");
    if (folded[1].kind === "tool") {
      expect(folded[1].tool.kind).toBe("execute");
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
