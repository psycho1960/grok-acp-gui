// GAG-010B / goal: mode ↔ workspace strategy linkage in the conversation.
// Drives the real shipped pure mapping, store, header control, and view.

import { createPinia, setActivePinia } from "pinia";
import { mount, flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { ModeInfo, TaskId } from "../../src/bridge/types";
import ConversationHeader from "../../src/features/conversation/ConversationHeader.vue";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import { useConversationStore } from "../../src/features/conversation/conversation-store";
import { fixtureSessionSnapshot, FIX_TASK } from "../../src/features/conversation/fixtures";
import {
  isWorkspaceStrategy,
  workspaceStrategyForMode,
} from "../../src/features/conversation/mode-workspace";

const MODES: ModeInfo[] = [
  { id: "agent", name: "智能体" },
  { id: "plan", name: "计划" },
  { id: "ask", name: "问答" },
];

describe("mode-workspace pure mapping", () => {
  it("links ask→direct and agent/plan→worktree, leaves readonly untouched", () => {
    expect(workspaceStrategyForMode("ask")).toBe("direct");
    expect(workspaceStrategyForMode("agent")).toBe("worktree");
    expect(workspaceStrategyForMode("plan")).toBe("worktree");
    expect(workspaceStrategyForMode(null)).toBeNull();
    expect(workspaceStrategyForMode("unknown")).toBeNull();
  });

  it("validates strategy values", () => {
    expect(isWorkspaceStrategy("worktree")).toBe(true);
    expect(isWorkspaceStrategy("readonly")).toBe(true);
    expect(isWorkspaceStrategy("direct")).toBe(true);
    expect(isWorkspaceStrategy("nonsense")).toBe(false);
    expect(isWorkspaceStrategy(42)).toBe(false);
    expect(isWorkspaceStrategy(null)).toBe(false);
  });
});

describe("conversation workspace strategy linkage", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
    sessionStorage.clear();
  });

  it("header renders the three Chinese strategy options", async () => {
    const wrapper = mount(ConversationHeader, {
      props: {
        title: "演示",
        status: "idle",
        modes: MODES,
        selectedWorkspaceStrategy: "worktree",
      },
    });
    const select = wrapper.get('[data-testid="conversation-workspace-select"]');
    expect(select.text()).toContain("工作区策略");
    const trigger = select.get('[data-testid="header-select-trigger"]');
    expect(trigger.attributes("data-selected-value")).toBe("worktree");
    await trigger.trigger("click");
    const options = select
      .findAll('[data-testid="header-select-option"]')
      .map((option) => option.text());
    expect(options).toEqual([
      "隔离 Worktree",
      "只读当前目录",
      "当前目录可写",
    ]);
    wrapper.unmount();
  });

  it("switching the mode emits the linked strategy and both persist", async () => {
    const configurePayloads: Array<Record<string, string>> = [];
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: { capabilities: { modes: MODES, models: [], slashCommands: [] } },
      onExecute(command) {
        if (command.type === "session.configure") {
          configurePayloads.push(command.payload.settings as Record<string, string>);
          return { success: "true", data: { acknowledged: "session.configure" } };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ status: "idle", mode: "agent", workspaceStrategy: "worktree" }),
    );

    // ask → direct linkage.
    await store.configureMode("ask");
    expect(configurePayloads).toEqual([{ mode: "ask", workspaceStrategy: "direct" }]);
    expect(store.workspaceStrategy).toBe("direct");
    // The header emits one atomic mode + strategy intent.
    const header = mount(ConversationHeader, {
      props: {
        title: "t",
        status: "idle",
        modes: MODES,
        selectedMode: "ask",
        selectedWorkspaceStrategy: "worktree",
      },
    });
    const modeSelect = header.get('[data-testid="conversation-mode-select"]');
    await modeSelect.get('[data-testid="header-select-trigger"]').trigger("click");
    await modeSelect.get('[data-value="plan"]').trigger("click");
    const emitted = header.emitted();
    expect(emitted["update:mode"]?.at(-1)).toEqual(["plan", "worktree"]);
    expect(emitted["update:workspaceStrategy"]).toBeUndefined();
    header.unmount();
  });

  it("store persists a manual strategy change and reverts on failure with a Chinese error", async () => {
    let fail = false;
    const configurePayloads: Array<Record<string, string>> = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "session.configure") {
          if (fail) {
            return {
              success: "false",
              error: {
                code: "DB_QUERY_FAILED",
                message: "保存失败",
                retryable: true,
                detailsRedacted: true,
                correlationId: "x" as never,
              },
            };
          }
          configurePayloads.push(command.payload.settings as Record<string, string>);
          return { success: "true", data: { acknowledged: "session.configure" } };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ status: "idle", workspaceStrategy: "worktree" }),
    );
    expect(store.workspaceStrategy).toBe("worktree");

    const ok = await store.configureWorkspaceStrategy("direct");
    expect(ok).toBe(true);
    expect(configurePayloads).toEqual([{ workspaceStrategy: "direct" }]);
    expect(store.workspaceStrategy).toBe("direct");

    fail = true;
    const bad = await store.configureWorkspaceStrategy("readonly");
    expect(bad).toBe(false);
    expect(store.workspaceStrategy).toBe("direct");
    expect(store.sendError).toBe("保存失败");
  });

  it("does not expose a new stable strategy before backend success", async () => {
    let finish: (() => void) | undefined;
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "session.configure") {
          return new Promise((resolve) => {
            finish = () => resolve({
              success: "true",
              data: { workspaceStrategy: "readonly", workspaceAvailable: true },
            });
          });
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ status: "idle", workspaceStrategy: "direct" }),
    );
    const pending = store.configureWorkspaceStrategy("readonly");
    expect(store.workspaceStrategy).toBe("direct");
    expect(store.settingsPending).toBe(true);
    expect(store.composerCapabilities.canSend).toBe(false);
    expect(store.composerCapabilities.disabledReason).toBe("正在保存会话设置…");
    finish?.();
    expect(await pending).toBe(true);
    expect(store.workspaceStrategy).toBe("readonly");
    expect(store.settingsPending).toBe(false);
  });

  it("invalidates a pending settings request when another task opens", async () => {
    let finish: (() => void) | undefined;
    const otherTask = "task-other" as TaskId;
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "session.configure") {
          return new Promise((resolve) => {
            finish = () => resolve({
              success: "true",
              data: { workspaceStrategy: "readonly", workspaceAvailable: true },
            });
          });
        }
        if (command.type === "task.open") {
          return {
            success: "true",
            data: {
              taskId: otherTask,
              title: "Other task",
              status: "idle",
              workspaceStrategy: "direct",
              workspaceAvailable: true,
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({ status: "idle", workspaceStrategy: "direct" }),
    );
    const pending = store.configureWorkspaceStrategy("readonly");
    expect(store.settingsPending).toBe(true);

    await store.openTask(otherTask);
    expect(store.settingsPending).toBe(false);
    expect(store.workspaceStrategy).toBe("direct");
    expect(store.composerCapabilities.canSend).toBe(true);

    finish?.();
    expect(await pending).toBe(false);
    expect(store.workspaceStrategy).toBe("direct");
  });

  it("restores the persisted strategy when reopening the task", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "task.open") {
          return {
            success: "true",
            data: {
              taskId: command.payload.taskId,
              title: "Reopened",
              status: "idle",
              mode: "ask",
              workspaceStrategy: "direct",
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle" }));
    expect(store.workspaceStrategy).toBeNull();

    await store.openTask(FIX_TASK);
    expect(store.workspaceStrategy).toBe("direct");
    expect(store.selectedMode).toBe("ask");
  });

  it("view wiring: mode switch drives both configure calls", async () => {
    const configureSettings: Array<Record<string, string>> = [];
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: { capabilities: { modes: MODES, models: [], slashCommands: [] } },
      onExecute(command) {
        if (command.type === "session.configure") {
          configureSettings.push(command.payload.settings as Record<string, string>);
          return { success: "true", data: { acknowledged: "session.configure" } };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const wrapper = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({
          status: "idle",
          cursor: 0,
          events: [],
          mode: "agent",
          workspaceStrategy: "worktree",
        }),
      },
      attachTo: document.body,
    });
    await flushPromises();

    const workspaceSelect = wrapper.get(
      '[data-testid="conversation-workspace-select"]',
    );
    await workspaceSelect
      .get('[data-testid="header-select-trigger"]')
      .trigger("click");
    expect(
      workspaceSelect
        .findAll('[data-testid="header-select-option"]')
        .map((option) => option.text()),
    ).toEqual([
      "隔离 Worktree",
      "只读当前目录",
      "当前目录可写",
    ]);

    // Switch mode ask → one atomic configure persists mode + direct strategy.
    const modeSelect = wrapper.get('[data-testid="conversation-mode-select"]');
    await modeSelect.get('[data-testid="header-select-trigger"]').trigger("click");
    await modeSelect.get('[data-value="ask"]').trigger("click");
    await flushPromises();
    expect(configureSettings).toEqual([{ mode: "ask", workspaceStrategy: "direct" }]);

    // Every displayed workspace option is actionable and persists independently.
    await workspaceSelect
      .get('[data-testid="header-select-trigger"]')
      .trigger("click");
    await workspaceSelect.get('[data-value="readonly"]').trigger("click");
    await flushPromises();
    expect(configureSettings).toEqual([
      { mode: "ask", workspaceStrategy: "direct" },
      { workspaceStrategy: "readonly" },
    ]);
    expect(wrapper.get('[data-testid="conversation-workspace-notice"]').text()).toContain(
      "只读策略已启用",
    );
    await workspaceSelect
      .get('[data-testid="header-select-trigger"]')
      .trigger("click");
    await workspaceSelect.get('[data-value="worktree"]').trigger("click");
    await flushPromises();
    await workspaceSelect
      .get('[data-testid="header-select-trigger"]')
      .trigger("click");
    await workspaceSelect.get('[data-value="direct"]').trigger("click");
    await flushPromises();
    expect(configureSettings.slice(-2)).toEqual([
      { workspaceStrategy: "worktree" },
      { workspaceStrategy: "direct" },
    ]);
    wrapper.unmount();
  });

  it("shows the fail-closed message when the backend reports a missing worktree", async () => {
    const bridge = createFakeDesktopBridge();
    const wrapper = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({
          status: "idle",
          cursor: 0,
          events: [],
          mode: "agent",
          workspaceStrategy: "worktree",
          workspaceAvailable: false,
        }),
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="conversation-workspace-notice"]').text()).toBe(
      "隔离 Worktree 尚未创建，本任务不会回落到原工作区。",
    );
    wrapper.unmount();
  });

  it("leaves loading and restores the draft when send is rejected for a missing worktree", async () => {
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        if (command.type === "turn.send") {
          return {
            success: "false",
            error: {
              code: "WORKTREE_NOT_READY",
              message: "managed worktree is unavailable",
              retryable: true,
              detailsRedacted: true,
              correlationId: "gag-010b-worktree-not-ready" as never,
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(
      fixtureSessionSnapshot({
        status: "idle",
        cursor: 0,
        events: [],
        mode: "agent",
        workspaceStrategy: "worktree",
        workspaceAvailable: false,
      }),
    );
    store.setDraft("不要丢失这段草稿");

    expect(await store.sendMessage()).toBe(false);
    expect(store.sendPending).toBe(false);
    expect(store.draft).toBe("不要丢失这段草稿");
    expect(store.sendError).toBe(
      "隔离 Worktree 尚未创建，本任务不会回落到原工作区。",
    );
  });
});
