import { createPinia, setActivePinia } from "pinia";
import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import { useConversationStore } from "../../src/features/conversation/conversation-store";
import {
  fixturePermission,
  fixturePlan,
  fixtureSessionSnapshot,
} from "../../src/features/conversation/fixtures";
import PermissionSlot from "../../src/features/conversation/slots/PermissionSlot.vue";
import PlanSlot from "../../src/features/conversation/slots/PlanSlot.vue";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import { applyEvents, createEmptyConversationState } from "../../src/features/conversation/reducer";

describe("GAG-009 permission and Plan UI", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
  });

  it("focuses the rejecting permission option and exposes operation impact", async () => {
    const event = fixturePermission(1);
    const state = applyEvents(createEmptyConversationState(event.taskId), [event]);
    const item = state.items[0];
    if (!item || item.kind !== "permission") throw new Error("permission fixture missing");
    const wrapper = mount(PermissionSlot, { props: { slotData: item.slot }, attachTo: document.body });
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("将修改工作区");
    expect(wrapper.text()).toContain("src/app.ts");
    expect(document.activeElement?.textContent).toContain("Reject");
    wrapper.unmount();
  });

  it("marks an older Plan version as superseded", () => {
    const state = applyEvents(createEmptyConversationState(), [fixturePlan(1), fixturePlan(2)]);
    const plans = state.items.filter((item) => item.kind === "plan");
    expect(plans).toHaveLength(2);
    expect(plans[0]?.kind === "plan" && plans[0].slot.approvalInvalidated).toBe(true);
    expect(plans[1]?.kind === "plan" && plans[1].slot.version).toBe(2);
  });

  it("renders numbered steps and disables invalidated Plan actions", () => {
    const event = fixturePlan(1);
    const state = applyEvents(createEmptyConversationState(event.taskId), [event]);
    const item = state.items[0];
    if (!item || item.kind !== "plan") throw new Error("plan fixture missing");
    const wrapper = mount(PlanSlot, {
      props: { slotData: { ...item.slot, approvalInvalidated: true } },
    });
    expect(wrapper.findAll("ol li")).toHaveLength(3);
    expect(wrapper.text()).toContain("批准已失效");
    expect(wrapper.findAll("button").every((button) => button.attributes("disabled") !== undefined)).toBe(true);
  });

  it("submits a permission only once during rapid double approval", async () => {
    let resolveCalls = 0;
    let release: (() => void) | undefined;
    const wait = new Promise<void>((resolve) => { release = resolve; });
    const bridge = createFakeDesktopBridge({
      async onExecute(command) {
        if (command.type === "permission.resolve") {
          resolveCalls += 1;
          await wait;
        }
        return { success: "true", data: { state: "approved_once" } };
      },
    });
    const store = useConversationStore();
    await store.attach(bridge);
    store.openFromSnapshot(fixtureSessionSnapshot({ cursor: 0, events: [], status: "running" }));
    store.injectEventForTest(fixturePermission(1));
    const item = store.items.find((candidate) => candidate.kind === "permission");
    if (!item || item.kind !== "permission") throw new Error("permission fixture missing");
    const first = store.resolvePermission(item.id, "allow-once-1");
    const second = store.resolvePermission(item.id, "allow-once-1");
    expect(await second).toBe(false);
    expect(resolveCalls).toBe(1);
    release?.();
    expect(await first).toBe(true);
  });

  it("renders redacted summaries instead of secret values", () => {
    const event = fixturePermission(1);
    const state = applyEvents(createEmptyConversationState(event.taskId), [event]);
    const item = state.items[0];
    if (!item || item.kind !== "permission") throw new Error("permission fixture missing");
    const wrapper = mount(PermissionSlot, { props: { slotData: item.slot }, attachTo: document.body });
    // Backend sends redacted summaries; the card must show the marker and
    // must never leak the test secret or a raw credential-shaped value.
    expect(wrapper.text()).toContain("[redacted]");
    expect(wrapper.text()).not.toContain("GAG009_TEST_SECRET_NEVER_LOG");
    expect(wrapper.text()).not.toMatch(/sk-[a-zA-Z0-9]{16,}|Bearer\s+[^\s]+/);
    wrapper.unmount();
  });

  it("keeps multiple pending requests isolated and rejects an expired one locally", async () => {
    const execute = vi.fn().mockResolvedValue({ success: "true", data: {} });
    const store = useConversationStore();
    await store.attach(createFakeDesktopBridge({ onExecute: execute }));
    store.openFromSnapshot(fixtureSessionSnapshot({ cursor: 0, events: [], status: "running" }));
    const first = fixturePermission(1, "permission-a");
    first.payload.expiresAtEpochSeconds = 1;
    store.injectEventForTest(first);
    store.injectEventForTest(fixturePermission(2, "permission-b"));
    const pending = store.items.filter((item) => item.kind === "permission");
    expect(pending).toHaveLength(2);
    expect(await store.resolvePermission(pending[0]!.id, "reject-1")).toBe(false);
    expect(execute).not.toHaveBeenCalled();
    expect(pending[1]!.kind === "permission" && pending[1].slot.requestId).toBe("permission-b");
  });

  it("Ctrl+. focuses the latest actionable approval", async () => {
    const permission = fixturePermission(1);
    const wrapper = mount(ConversationView, {
      props: {
        bridge: createFakeDesktopBridge(),
        snapshot: fixtureSessionSnapshot({ cursor: 1, events: [permission] }),
      },
      global: { plugins: [createPinia()] },
      attachTo: document.body,
    });
    await vi.waitFor(() => {
      expect(wrapper.find('[data-testid="permission-slot"]').exists()).toBe(true);
    });
    (document.activeElement as HTMLElement | null)?.blur();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: ".", ctrlKey: true }));
    expect(document.activeElement?.textContent).toContain("Reject");
    wrapper.unmount();
  });
});
