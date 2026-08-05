import { describe, expect, it } from "vitest";
import { isValidDesktopEvent } from "../../src/bridge/client";

describe("GAG-008 DesktopBridge event isolation", () => {
  it("rejects session events without a complete task/session/sequence scope", () => {
    const base = {
      type: "message.delta",
      timestamp: "2026-08-05T00:00:00.000Z",
      payload: { text: "must not leak" },
    };
    expect(isValidDesktopEvent(base)).toBe(false);
    expect(isValidDesktopEvent({ ...base, taskId: "task-a" })).toBe(false);
    expect(
      isValidDesktopEvent({
        ...base,
        taskId: "task-a",
        sessionId: "session-a",
        seq: 1,
      }),
    ).toBe(true);
  });

  it("accepts a structurally valid non-session runtime event", () => {
    expect(
      isValidDesktopEvent({
        type: "runtime.updated",
        timestamp: "2026-08-05T00:00:00.000Z",
        payload: { status: "ready" },
      }),
    ).toBe(true);
  });
});
