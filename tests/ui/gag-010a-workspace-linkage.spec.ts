// GAG-010A / goal: mode ↔ workspace strategy linkage in the conversation.
// Drives the real shipped pure mapping, store, header control, and view.

import { createPinia, setActivePinia } from "pinia";
import { mount, flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { ModeInfo } from "../../src/bridge/types";
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

  it("header renders the three Chinese strategy options", () => {
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
    const options = select.findAll("option").map((option) => option.text());
    expect(options).toEqual([
      "使用创建时的策略",
      "隔离 Worktree",
      "只读当前目录",
      "当前目录可写",
    ]);
    expect((select.get("select").element as HTMLSelectElement).value).toBe("worktree");
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
    expect(configurePayloads).toEqual([{ mode: "ask" }]);
    // The store itself does not link (the header emits both); the header does.
    const header = mount(ConversationHeader, {
      props: {
        title: "t",
        status: "idle",
        modes: MODES,
        selectedMode: "ask",
        selectedWorkspaceStrategy: "worktree",
      },
    });
    await header.get('[data-testid="conversation-mode-select"] select').setValue("plan");
    const emitted = header.emitted();
    expect(emitted["update:mode"]?.at(-1)).toEqual(["plan"]);
    expect(emitted["update:workspaceStrategy"]?.at(-1)).toEqual(["worktree"]);
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
      '[data-testid="conversation-workspace-select"] select',
    );
    expect(workspaceSelect.findAll("option").map((o) => o.text())).toEqual([
      "使用创建时的策略",
      "隔离 Worktree",
      "只读当前目录",
      "当前目录可写",
    ]);

    // Switch mode ask → the header links workspaceStrategy to direct.
    const modeSelect = wrapper.get('[data-testid="conversation-mode-select"] select');
    await modeSelect.setValue("ask");
    await flushPromises();
    expect(configureSettings).toEqual([{ mode: "ask" }, { workspaceStrategy: "direct" }]);

    // Manual strategy change persists independently.
    await workspaceSelect.setValue("readonly");
    await flushPromises();
    expect(configureSettings).toEqual([
      { mode: "ask" },
      { workspaceStrategy: "direct" },
      { workspaceStrategy: "readonly" },
    ]);
    wrapper.unmount();
  });
});
