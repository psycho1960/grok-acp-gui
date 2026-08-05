import { beforeEach, describe, expect, it, vi } from "vitest";

describe("GAG-008 durable per-task UI recovery", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    vi.resetModules();
  });

  it("restores an unsent draft after the page session is destroyed", async () => {
    const first = await import("../../src/features/conversation/draft");
    first.saveDraft("task-durable", "unsent after restart");

    // A desktop-app restart destroys sessionStorage and all module state.
    sessionStorage.clear();
    vi.resetModules();

    const restarted = await import("../../src/features/conversation/draft");
    expect(restarted.loadDraft("task-durable")).toBe("unsent after restart");
  });

  it("restores each session scroll anchor after all module state is lost", async () => {
    const first = await import("../../src/features/conversation/scroll");
    first.saveScrollAnchor("session-a", {
      scrollTop: 480,
      anchorEventKey: "session-a:24",
      stickToBottom: false,
      unreadCount: 3,
    });
    first.saveScrollAnchor("session-b", {
      scrollTop: 90,
      anchorEventKey: "session-b:4",
      stickToBottom: false,
      unreadCount: 0,
    });

    vi.resetModules();
    const restarted = await import("../../src/features/conversation/scroll");

    expect(restarted.loadScrollAnchor("session-a")).toEqual({
      scrollTop: 480,
      anchorEventKey: "session-a:24",
      stickToBottom: false,
      unreadCount: 3,
    });
    expect(restarted.loadScrollAnchor("session-b").scrollTop).toBe(90);
  });
});
