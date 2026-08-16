import { createPinia, setActivePinia } from "pinia";
import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import Composer from "../../src/features/conversation/Composer.vue";
import ConversationHeader from "../../src/features/conversation/ConversationHeader.vue";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import SafeMarkdown from "../../src/features/conversation/SafeMarkdown.vue";
import TimelineVirtualList from "../../src/features/conversation/TimelineVirtualList.vue";
import ToolCard from "../../src/features/conversation/ToolCard.vue";
import {
  fixtureSessionSnapshot,
  generateManyEvents,
} from "../../src/features/conversation/fixtures";
import { applyEvents, createEmptyConversationState } from "../../src/features/conversation/reducer";
import { FIX_TASK } from "../../src/features/conversation/fixtures";
import type { TimelineItem, ToolCallView } from "../../src/features/conversation/types";

describe("GAG-008 components", () => {
  it("SafeMarkdown does not inject script tags", () => {
    const w = mount(SafeMarkdown, {
      props: { source: '<img src=x onerror=alert(1)> **ok**' },
    });
    expect(w.html()).not.toMatch(/<img /i);
    expect(w.html()).toContain("&lt;img");
    expect(w.html()).toContain("<strong>ok</strong>");
  });

  it("SafeMarkdown renders structured assistant content", () => {
    const w = mount(SafeMarkdown, {
      props: {
        source: "## 技术栈\n\n| 层 | 技术 |\n| --- | --- |\n| 前端 | 微信小程序 |\n\n1. 开发版\n2. 正式版\n\n- Fastify\n- SQLite",
      },
    });

    expect(w.get("h2").text()).toBe("技术栈");
    expect(w.get("table").text()).toContain("微信小程序");
    expect(w.findAll("ol > li")).toHaveLength(2);
    expect(w.findAll("ul > li")).toHaveLength(2);
  });

  it("copies the complete visible message with code preserved and secrets redacted", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const longTail = "z".repeat(5_000);
    const w = mount(SafeMarkdown, {
      props: {
        source: `before\n\n\`\`\`ts\nconst answer = 42;\n\`\`\`\n\nTOKEN=do-not-copy\n${longTail}`,
      },
    });

    await w.get('[data-testid="copy-message"]').trigger("click");
    expect(writeText).toHaveBeenCalledOnce();
    const copied = String(writeText.mock.calls[0]?.[0]);
    expect(copied).toContain("const answer = 42;");
    expect(copied).not.toContain("do-not-copy");
    expect(copied).toContain("[redacted]");
    expect(copied.endsWith(longTail)).toBe(true);
  });

  it("ToolCard shows phase, duration, redaction badge", () => {
    const tool: ToolCallView = {
      toolCallId: "t1",
      title: "Shell",
      kind: "execute",
      phase: "completed",
      durationMs: 1500,
      locations: ["a.ts"],
      input: { summary: "[redacted]", redacted: true },
      result: { summary: "exit 0", redacted: false },
      exitCode: 0,
    };
    const w = mount(ToolCard, { props: { tool, expanded: true } });
    expect(w.get('[data-testid="tool-duration"]').text()).toContain("1.5s");
    expect(w.text()).toContain("已脱敏");
    expect(w.get('[data-testid="tool-details"]').exists()).toBe(true);
  });

  it("Composer emits send on Enter and cancel on Escape", async () => {
    const w = mount(Composer, {
      props: {
        modelValue: "hi",
        capabilities: {
          canSend: true,
          canCancel: true,
          bridgeOnline: true,
        },
      },
    });
    await w.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    expect(w.emitted("send")).toBeTruthy();
    await w.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Escape",
    });
    expect(w.emitted("cancel")).toBeTruthy();
  });

  it("ConversationHeader offers an explicit recovery action after failure", async () => {
    const w = mount(ConversationHeader, {
      props: { title: "Failed turn", status: "error" },
    });
    await w.get('[data-testid="resume-session"]').trigger("click");
    expect(w.emitted("resume")).toBeTruthy();
  });

  it("virtual list does not render 10k DOM nodes", () => {
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, generateManyEvents(10_000));
    const w = mount(TimelineVirtualList, {
      props: {
        items: state.items,
        itemHeight: 80,
        sessionKey: "perf",
      },
      slots: {
        default: `<div class="row">row</div>`,
      },
      attachTo: document.body,
    });
    // Force viewport size
    const root = w.get('[data-testid="conversation-virtual-list"]').element as HTMLElement;
    Object.defineProperty(root, "clientHeight", { value: 400, configurable: true });
    root.dispatchEvent(new Event("scroll"));
    const rows = w.findAll(".virtual-row");
    expect(rows.length).toBeLessThan(100);
    expect(rows.length).toBeGreaterThan(0);
    const spacer = w.get('[data-testid="conversation-virtual-spacer"]');
    expect(spacer.attributes("style")).toMatch(/height/);
    w.unmount();
  });

  it("ConversationView mounts with snapshot fixtures", async () => {
    setActivePinia(createPinia());
    const bridge = createFakeDesktopBridge();
    const w = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot(),
      },
      attachTo: document.body,
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(w.get('[data-testid="conversation-view"]').exists()).toBe(true);
    expect(w.get('[data-testid="conversation-header"]').exists()).toBe(true);
    expect(w.get('[data-testid="composer"]').exists()).toBe(true);
    w.unmount();
  });

  it("counts streaming growth as unread while the user is reading history", async () => {
    const item: TimelineItem = {
      id: "assistant-stream",
      kind: "assistant",
      seq: 1,
      sessionId: "session-scroll",
      timestamp: "2026-08-05T00:00:00.000Z",
      eventKey: "session-scroll:1",
      text: "first",
      streaming: true,
      frozen: false,
    };
    const w = mount(TimelineVirtualList, {
      props: { items: [item], sessionKey: "stream-unread" },
      slots: { default: `<div>row</div>` },
      attachTo: document.body,
    });
    const root = w.get('[data-testid="conversation-virtual-list"]').element as HTMLElement;
    Object.defineProperty(root, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(root, "scrollHeight", { value: 1000, configurable: true });
    root.scrollTop = 0;
    root.dispatchEvent(new Event("scroll"));

    await w.setProps({ items: [{ ...item, text: "first plus streamed text", seq: 2 }] });
    await Promise.resolve();
    expect(w.get('[data-testid="unread-count"]').text()).toBe("1");
    expect(root.scrollTop).toBe(0);
    w.unmount();
  });

  it("follows appended content only while the viewport is at the bottom", async () => {
    const first: TimelineItem = {
      id: "bottom-first",
      kind: "assistant",
      seq: 1,
      sessionId: "session-bottom",
      timestamp: "2026-08-05T00:00:00.000Z",
      eventKey: "session-bottom:1",
      text: "first",
      streaming: true,
      frozen: false,
    };
    const second: TimelineItem = {
      id: "bottom-second",
      kind: "system",
      seq: 2,
      sessionId: "session-bottom",
      timestamp: "2026-08-05T00:00:01.000Z",
      eventKey: "session-bottom:2",
      message: "second",
    };
    const w = mount(TimelineVirtualList, {
      props: { items: [first], sessionKey: "follow-bottom" },
      slots: { default: `<div>row</div>` },
      attachTo: document.body,
    });
    const root = w.get('[data-testid="conversation-virtual-list"]').element as HTMLElement;
    let physicalHeight = 500;
    Object.defineProperty(root, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(root, "scrollHeight", {
      configurable: true,
      get: () => physicalHeight,
    });
    root.scrollTop = 300;
    root.dispatchEvent(new Event("scroll"));

    physicalHeight = 700;
    await w.setProps({ items: [first, second] });
    await Promise.resolve();
    await Promise.resolve();

    expect(root.scrollTop).toBe(500);
    // Button stays mounted for transition; hidden while stick-to-bottom.
    expect(w.get('[data-testid="jump-to-bottom"]').classes()).not.toContain("visible");
    w.unmount();
  });

  it("restores the actual viewport when switching away from and back to a session", async () => {
    const events = generateManyEvents(80);
    let state = createEmptyConversationState(FIX_TASK);
    state = applyEvents(state, events);
    const w = mount(TimelineVirtualList, {
      props: { items: state.items, sessionKey: "switch-session-a", itemHeight: 80 },
      slots: { default: `<div>row</div>` },
      attachTo: document.body,
    });
    const root = w.get('[data-testid="conversation-virtual-list"]').element as HTMLElement;
    Object.defineProperty(root, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(root, "scrollHeight", { value: 2_000, configurable: true });

    root.scrollTop = 480;
    root.dispatchEvent(new Event("scroll"));
    await w.setProps({ sessionKey: "switch-session-b" });
    await Promise.resolve();

    root.scrollTop = 90;
    root.dispatchEvent(new Event("scroll"));
    await w.setProps({ sessionKey: "switch-session-a" });
    await Promise.resolve();
    await Promise.resolve();

    expect(root.scrollTop).toBe(480);
    w.unmount();
  });
});
