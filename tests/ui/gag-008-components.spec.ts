import { createPinia, setActivePinia } from "pinia";
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import Composer from "../../src/features/conversation/Composer.vue";
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
import type { ToolCallView } from "../../src/features/conversation/types";

describe("GAG-008 components", () => {
  it("SafeMarkdown does not inject script tags", () => {
    const w = mount(SafeMarkdown, {
      props: { source: '<img src=x onerror=alert(1)> **ok**' },
    });
    expect(w.html()).not.toMatch(/<img /i);
    expect(w.html()).toContain("&lt;img");
    expect(w.html()).toContain("<strong>ok</strong>");
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
});
