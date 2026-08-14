import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge, fakeError } from "../../src/bridge/fake-bridge";
import type { DesktopCommand } from "../../src/bridge/types";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import {
  FIX_TASK,
  fixturePermission,
  fixtureSessionSnapshot,
  fixtureTaskState,
} from "../../src/features/conversation/fixtures";
import { useConversationStore } from "../../src/features/conversation/conversation-store";

async function mountRunningConversation(onExecute?: (command: DesktopCommand) => unknown) {
  const commands: DesktopCommand[] = [];
  const bridge = createFakeDesktopBridge({
    onExecute(command) {
      commands.push(command);
      const extra = onExecute?.(command);
      if (extra) return extra;
      return { success: "true", data: { acknowledged: command.type } };
    },
  });
  const wrapper = mount(ConversationView, {
    props: {
      bridge,
      taskId: FIX_TASK,
      snapshot: fixtureSessionSnapshot({
        status: "running",
        cursor: 0,
        events: [],
        items: [],
      }),
    },
    attachTo: document.body,
  });
  await flushPromises();
  return { wrapper, commands, bridge };
}

describe("GAG-021 queue and interrupt", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("queues Enter while running and does not send", async () => {
    const { wrapper, commands } = await mountRunningConversation();
    const store = useConversationStore();
    store.setDraft("follow up later");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="queue-bar"]').text()).toContain("follow up later");
    expect(commands.some((command) => command.type === "turn.send")).toBe(false);
    expect(store.draft).toBe("");
    wrapper.unmount();
  });

  it("orders queue actions as edit, send now, delete", async () => {
    const { wrapper } = await mountRunningConversation();
    const store = useConversationStore();
    store.setDraft("queued");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    const actions = wrapper.get('[data-testid="queue-item"]').findAll("button");
    expect(actions.map((button) => button.attributes("data-testid"))).toEqual([
      "queue-edit",
      "queue-send-now",
      "queue-delete",
    ]);
    await wrapper.get('[data-testid="queue-delete"]').trigger("click");
    expect(wrapper.find('[data-testid="queue-bar"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("edit restores text and send-now cancels then sends", async () => {
    const { wrapper, commands } = await mountRunningConversation();
    const store = useConversationStore();
    store.setDraft("edit me");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    await wrapper.get('[data-testid="queue-edit"]').trigger("click");
    expect(store.draft).toBe("edit me");
    expect(wrapper.find('[data-testid="queue-bar"]').exists()).toBe(false);

    store.setDraft("send now");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    await wrapper.get('[data-testid="queue-send-now"]').trigger("click");
    await flushPromises();
    expect(commands.some((command) => command.type === "turn.cancel")).toBe(true);
    store.injectEventForTest(fixtureTaskState(1, "idle"));
    await flushPromises();
    expect(commands.some((command) => command.type === "turn.send")).toBe(true);
    const sent = commands.find((command) => command.type === "turn.send");
    expect(sent && "payload" in sent ? sent.payload : null).toMatchObject({ message: "send now" });
    wrapper.unmount();
  });

  it("Stop cancels without sending and keeps the dock draft", async () => {
    const { wrapper, commands } = await mountRunningConversation();
    const store = useConversationStore();
    store.setDraft("keep this draft");
    await flushPromises();
    await wrapper.get('[data-testid="composer-stop"]').trigger("click");
    await flushPromises();
    expect(commands.some((command) => command.type === "turn.cancel")).toBe(true);
    expect(commands.some((command) => command.type === "turn.send")).toBe(false);
    expect(store.draft).toBe("keep this draft");
    wrapper.unmount();
  });

  it("a flushed queued turn still shows permission cards with original option IDs", async () => {
    const { wrapper } = await mountRunningConversation();
    const store = useConversationStore();
    store.setDraft("need permission next");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    store.injectEventForTest(fixtureTaskState(1, "idle"));
    await flushPromises();
    store.injectEventForTest(fixturePermission(2));
    await flushPromises();
    const slot = wrapper.get('[data-testid="permission-slot"]');
    expect(slot.text()).toContain("允许一次");
    expect(slot.text()).toContain("拒绝");
    const permission = store.items.find((item) => item.kind === "permission");
    expect(permission?.kind === "permission" && permission.slot.options.map((option) => option.optionId)).toEqual([
      "allow-once-1",
      "reject-1",
    ]);
    expect(slot.find("[data-safe-default='true']").exists()).toBe(true);
    wrapper.unmount();
  });

  it("keeps the first send-now item when a second send-now is clicked mid-cancel", async () => {
    let releaseCancel: (() => void) | undefined;
    const cancelGate = new Promise<void>((resolve) => {
      releaseCancel = resolve;
    });
    const { wrapper, commands } = await mountRunningConversation(async (command) => {
      if (command.type === "turn.cancel") await cancelGate;
      return undefined;
    });
    const store = useConversationStore();
    store.setDraft("follow-up A");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    store.setDraft("follow-up B");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    await flushPromises();
    expect(store.queuedFollowUps.map((item) => item.text)).toEqual([
      "follow-up A",
      "follow-up B",
    ]);

    const first = store.sendFollowUpNow(store.queuedFollowUps[0]!.id);
    await flushPromises();
    expect(wrapper.get('[data-testid="queue-send-now"]').attributes("disabled")).toBeDefined();
    const secondId = store.queuedFollowUps[0]!.id;
    const second = await store.sendFollowUpNow(secondId);
    expect(second).toBe(false);
    expect(store.queuedFollowUps.map((item) => item.text)).toEqual(["follow-up B"]);

    releaseCancel?.();
    await first;
    store.injectEventForTest(fixtureTaskState(1, "idle"));
    await flushPromises();
    const sent = commands.filter((command) => command.type === "turn.send");
    expect(sent).toHaveLength(1);
    expect(sent[0] && "payload" in sent[0] ? sent[0].payload : null).toMatchObject({
      message: "follow-up A",
    });
    expect(store.queuedFollowUps.map((item) => item.text)).toEqual(["follow-up B"]);
    wrapper.unmount();
  });

  it("restores the composer draft when a flushed queued send fails", async () => {
    const { wrapper } = await mountRunningConversation((command) => {
      if (command.type === "turn.send") {
        return { success: "false", error: fakeError({ message: "发送被拒绝" }) };
      }
      return undefined;
    });
    const store = useConversationStore();
    store.setDraft("queued body");
    await flushPromises();
    await wrapper.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: false,
    });
    store.setDraft("composer draft");
    await flushPromises();
    store.injectEventForTest(fixtureTaskState(1, "idle"));
    await flushPromises();
    expect(store.draft).toBe("composer draft");
    expect(store.sendError).toContain("发送被拒绝");
    wrapper.unmount();
  });
});
