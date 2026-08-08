// GAG-011 / goal: in-conversation mode switching (智能体/计划/问答).
// Drives the real shipped store, header control, and view wiring.

import { createPinia, setActivePinia } from "pinia";
import { mount, flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopCommand, ModeInfo } from "../../src/bridge/types";
import ConversationHeader from "../../src/features/conversation/ConversationHeader.vue";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import { useConversationStore } from "../../src/features/conversation/conversation-store";
import { fixtureSessionSnapshot, FIX_TASK } from "../../src/features/conversation/fixtures";

const MODES: ModeInfo[] = [
  { id: "agent", name: "智能体" },
  { id: "plan", name: "计划" },
  { id: "ask", name: "问答" },
];

describe("conversation mode switching", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
    sessionStorage.clear();
  });

  it("header renders the three Chinese mode options from capability modes", () => {
    const wrapper = mount(ConversationHeader, {
      props: {
        title: "演示",
        status: "idle",
        modes: MODES,
        selectedMode: "agent",
      },
    });
    const select = wrapper.get('[data-testid="conversation-mode-select"]');
    expect(select.text()).toContain("模式");
    const options = select.findAll("option").map((option) => option.text());
    expect(options).toEqual(["使用会话默认模式", "智能体", "计划", "问答"]);
    expect((select.get("select").element as HTMLSelectElement).value).toBe("agent");
    wrapper.unmount();
  });

  it("switching the mode persists via session.configure with the mode payload", async () => {
    const configurePayloads: Array<Record<string, string>> = [];
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: { capabilities: { modes: MODES, models: [], slashCommands: [] } },
      onExecute(command) {
        if (command.type === "session.configure") {
          configurePayloads.push(
            command.payload.settings as Record<string, string>,
          );
          return { success: "true", data: { acknowledged: "session.configure" } };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle", mode: "agent" }));
    expect(store.selectedMode).toBe("agent");

    await store.configureMode("plan");
    expect(configurePayloads).toEqual([{ mode: "plan" }]);
    expect(store.selectedMode).toBe("plan");

    // Clearing back to the session default sends null.
    await store.configureMode(null);
    expect(configurePayloads).toEqual([{ mode: "plan" }, { mode: null }]);
    expect(store.selectedMode).toBeNull();
  });

  it("restores the persisted mode when reopening the task", async () => {
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
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle" }));
    expect(store.selectedMode).toBeNull();

    await store.openTask(FIX_TASK);
    expect(store.selectedMode).toBe("ask");
  });

  it("reverts the local selection with a Chinese error when configure fails", async () => {
    let fail = false;
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
          return { success: "true", data: {} };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ status: "idle", mode: "agent" }));

    await store.configureMode("ask");
    expect(store.selectedMode).toBe("ask");

    fail = true;
    const ok = await store.configureMode("plan");
    expect(ok).toBe(false);
    expect(store.selectedMode).toBe("ask");
    expect(store.sendError).toBe("保存失败");
  });

  it("view wiring: header select drives the store and the echo carries the mode", async () => {
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: { capabilities: { modes: MODES, models: [], slashCommands: [] } },
      onExecute(command) {
        commands.push(command);
        if (command.type === "session.configure") {
          return { success: "true", data: { acknowledged: "session.configure" } };
        }
        if (command.type === "turn.send") {
          return { success: "true", data: { seq: 1 } };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const wrapper = mount(ConversationView, {
      props: {
        bridge,
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({ status: "idle", cursor: 0, events: [], mode: "agent" }),
      },
      attachTo: document.body,
    });
    await flushPromises();

    const modeSelect = wrapper.get(
      '[data-testid="conversation-mode-select"] select',
    );
    expect(modeSelect.findAll("option").map((o) => o.text())).toEqual([
      "使用会话默认模式",
      "智能体",
      "计划",
      "问答",
    ]);
    await modeSelect.setValue("plan");
    await flushPromises();
    expect(
      commands.some(
        (c) =>
          c.type === "session.configure" &&
          (c.payload as { settings: Record<string, string> }).settings.mode === "plan",
      ),
    ).toBe(true);

    const store = useConversationStore();
    expect(store.selectedMode).toBe("plan");
    wrapper.unmount();
  });
});
