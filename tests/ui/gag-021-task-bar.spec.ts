import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { ModeInfo } from "../../src/bridge/types";
import ConversationHeader from "../../src/features/conversation/ConversationHeader.vue";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import { FIX_TASK, fixtureSessionSnapshot } from "../../src/features/conversation/fixtures";
import ShellView from "../../src/app/ShellView.vue";


const MODES: ModeInfo[] = [
  { id: "agent", name: "智能体" },
  { id: "plan", name: "计划" },
  { id: "ask", name: "问答" },
];

describe("GAG-021 task bar and breadcrumb", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("does not label the first attempt", () => {
    const first = mount(ConversationHeader, {
      props: { title: "修对比度", status: "idle", attempt: 1 },
    });
    expect(first.text()).not.toContain("第 1 次尝试");
    first.unmount();

    const retry = mount(ConversationHeader, {
      props: { title: "修对比度", status: "idle", attempt: 2 },
    });
    expect(retry.text()).toContain("第 2 次尝试");
    retry.unmount();
  });

  it("shows conversation status once on the task bar", () => {
    const w = mount(ConversationHeader, {
      props: { title: "修对比度", status: "idle" },
    });
    expect(w.get('[data-testid="conversation-status"]').text()).toBe("空闲");
    expect(w.text().split("空闲").length - 1).toBe(1);
    w.unmount();
  });

  it("idle mode and workspace badges keep a chevron menu", () => {
    const w = mount(ConversationHeader, {
      props: {
        title: "修对比度",
        status: "idle",
        modes: MODES,
        selectedMode: "agent",
        selectedWorkspaceStrategy: "worktree",
      },
    });
    expect(w.find('[data-testid="mode-chevron"]').exists()).toBe(true);
    expect(w.find('[data-testid="workspace-chevron"]').exists()).toBe(true);
    expect(w.get('[data-testid="conversation-mode-select"] select').exists()).toBe(true);
    w.unmount();
  });

  it("locks mode and workspace badges without a chevron while running", () => {
    const w = mount(ConversationHeader, {
      props: {
        title: "修对比度",
        status: "running",
        modes: MODES,
        selectedMode: "agent",
        selectedWorkspaceStrategy: "worktree",
        settingsDisabled: true,
      },
    });
    expect(w.find('[data-testid="mode-chevron"]').exists()).toBe(false);
    expect(w.find('[data-testid="workspace-chevron"]').exists()).toBe(false);
    expect(
      (w.get('[data-testid="conversation-mode-select"] select').element as HTMLSelectElement)
        .disabled,
    ).toBe(true);
    expect(w.get('[data-testid="conversation-mode-select"]').attributes("title")).toContain("运行");
    expect(w.get('[data-testid="conversation-workspace-select"]').attributes("title")).toContain("运行");
    w.unmount();

    const waiting = mount(ConversationHeader, {
      props: {
        title: "修对比度",
        status: "waiting_permission",
        modes: MODES,
        selectedMode: "agent",
        settingsDisabled: true,
      },
    });
    expect(waiting.find('[data-testid="mode-chevron"]').exists()).toBe(false);
    expect(waiting.get('[data-testid="conversation-mode-select"]').attributes("title")).toContain("审批");
    waiting.unmount();
  });

  it("back is a 32px chevron that returns to the task center", async () => {
    window.location.hash = `#conversation/${FIX_TASK}`;
    const w = mount(ConversationView, {
      props: {
        bridge: createFakeDesktopBridge(),
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({ status: "idle", cursor: 0, events: [], items: [] }),
      },
      attachTo: document.body,
    });
    await flushPromises();
    const back = w.get('[data-testid="conversation-back"]');
    expect(back.attributes("aria-label")).toMatch(/返回|任务中心/);
    expect(back.classes()).toContain("icon-button");
    await back.trigger("click");
    expect(window.location.hash).toMatch(/task-center/);
    w.unmount();
  });

  it("breadcrumb is project / 对话 and never repeats the task title", async () => {
    window.location.hash = `#conversation/${FIX_TASK}`;
    const w = mount(ShellView, {
      global: { plugins: [createPinia()] },
      attachTo: document.body,
    });
    await flushPromises();
    const crumb = w.get('[data-testid="topbar-breadcrumb"]');
    expect(crumb.text().replace(/\s+/g, " ")).toMatch(/\/\s*对话\s*$/);
    expect(crumb.text()).not.toContain("对话：");
    w.unmount();
  });
});
