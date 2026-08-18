import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { nextTick } from "vue";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { SessionId, TaskId, TaskOpenResult } from "../../src/bridge/types";
import TaskCenterView from "../../src/features/task-center/TaskCenterView.vue";
import VirtualList from "../../src/features/task-center/VirtualList.vue";
import { createTaskCenterSeedSnapshot } from "../../src/features/task-center/seed";
import { useTaskCenterStore } from "../../src/features/task-center/task-center-store";

function createBridge(options?: { failCancel?: boolean }) {
  const snapshot = createTaskCenterSeedSnapshot();
  return createFakeDesktopBridge({
    bootstrapSnapshot: snapshot,
    onExecute(command) {
      if (command.type === "task.open") {
        const task = snapshot.activeTasks?.find((t) => t.id === command.payload.taskId);
        if (!task) {
          return {
            success: "false",
            error: {
              code: "TEST",
              message: "missing",
              retryable: false,
              detailsRedacted: true,
              correlationId: "c" as never,
            },
          };
        }
        const data: TaskOpenResult = {
          taskId: task.id,
          title: task.title,
          status: task.status,
        };
        return { success: "true", data };
      }
      if (command.type === "turn.cancel") {
        if (options?.failCancel) {
          return {
            success: "false",
            error: {
              code: "TEST",
              message: "取消失败：后端拒绝",
              retryable: true,
              detailsRedacted: true,
              correlationId: "c" as never,
            },
          };
        }
        return { success: "true", data: { acknowledged: "turn.cancel" } };
      }
      return { success: "true", data: { acknowledged: command.type } };
    },
  });
}

describe("GAG-007 TaskCenterView", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("renders loading then task list with accessible name", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    expect(wrapper.find('[data-testid="task-loading"]').exists()).toBe(true);
    await flushPromises();
    await nextTick();
    expect(wrapper.find('[data-testid="task-center"]').exists()).toBe(true);
    expect(wrapper.get("#task-center-title").text()).toContain("任务中心");
    expect(
      wrapper.find('[data-testid="task-list"]').exists() ||
        wrapper.find('[data-testid="virtual-list"]').exists(),
    ).toBe(true);
    expect(wrapper.findAll("[data-task-id]").length).toBeGreaterThan(0);
    expect(wrapper.find("[data-group-header]").exists()).toBe(true);
    wrapper.unmount();
  });

  it("shows empty state when bootstrap has no tasks", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: {
        ...createTaskCenterSeedSnapshot(),
        activeTasks: [],
        completedTasks: [],
      },
    });
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    expect(wrapper.find('[data-testid="task-empty"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("shows error state when bridge bootstrap fails", async () => {
    const bridge = createFakeDesktopBridge();
    bridge.bootstrap = async () => {
      throw new Error("boom");
    };
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    expect(wrapper.find('[data-testid="task-error"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("boom");
    wrapper.unmount();
  });

  it("opens detail drawer from open control and closes via Escape on dialog", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    await wrapper
      .get('[data-task-id="task-run-1"] [data-testid="task-open"]')
      .trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="task-detail"]').exists()).toBe(true);
    const dialog = wrapper.get('[role="dialog"]');
    await dialog.trigger("keydown", { key: "Escape" });
    await nextTick();
    await nextTick();
    expect(wrapper.find('[data-testid="task-detail"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("opens detail drawer when clicking the non-action area of a task card", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();

    await wrapper
      .get('[data-task-id="task-run-1"] .task-meta')
      .trigger("click");
    await flushPromises();

    expect(wrapper.find('[data-testid="task-detail"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("opens an existing task conversation from the detail drawer", async () => {
    window.location.hash = "#task-center";
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: true },
      attachTo: document.body,
    });
    await flushPromises();
    await wrapper
      .get('[data-task-id="task-run-1"] [data-testid="task-open"]')
      .trigger("click");
    await flushPromises();

    await wrapper.get('[data-testid="detail-open-conversation"]').trigger("click");

    expect(window.location.hash).toBe("#conversation/task-run-1");
    wrapper.unmount();
    window.location.hash = "";
  });

  it("filters by search query locally", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const input = wrapper.get('[data-testid="task-search"] input');
    await input.setValue("中断");
    await nextTick();
    const ids = wrapper.findAll("[data-task-id]").map((n) => n.attributes("data-task-id"));
    expect(ids).toEqual(["task-int-1"]);
    wrapper.unmount();
  });

  it("filters by group chip and shows filtered-empty", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    await wrapper.get('[data-testid="toggle-filters"]').trigger("click");
    await nextTick();
    const groupSelect = wrapper.get('[data-testid="task-filter-group"] select');
    await groupSelect.setValue("failed_interrupted");
    await nextTick();
    expect(wrapper.findAll("[data-task-id]").map((n) => n.attributes("data-task-id"))).toEqual([
      "task-int-1",
    ]);

    const statusSelect = wrapper.get('[data-testid="task-filter-status"] select');
    await statusSelect.setValue("merged");
    await nextTick();
    expect(wrapper.find('[data-testid="task-filtered-empty"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("hash without group clears previous group filter to all", async () => {
    window.location.hash = "#task-center?group=failed_interrupted";
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: true },
      attachTo: document.body,
    });
    await flushPromises();
    await nextTick();
    const store = useTaskCenterStore();
    expect(store.filters.group).toBe("failed_interrupted");
    expect(wrapper.findAll("[data-task-id]").map((n) => n.attributes("data-task-id"))).toEqual([
      "task-int-1",
    ]);

    window.location.hash = "#task-center";
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    await nextTick();
    await flushPromises();
    expect(store.filters.group).toBe("all");
    expect(wrapper.findAll("[data-task-id]").length).toBeGreaterThan(1);
    wrapper.unmount();
    window.location.hash = "";
  });

  it("shows stale banner with list retained and retry", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const store = useTaskCenterStore();
    store.markStale("disconnected");
    await nextTick();
    expect(wrapper.find('[data-testid="task-stale-banner"]').exists()).toBe(true);
    expect(wrapper.findAll("[data-task-id]").length).toBeGreaterThan(0);
    await wrapper.get('[data-testid="task-retry"]').trigger("click");
    await flushPromises();
    expect(store.loadState).toBe("ready");
    wrapper.unmount();
  });

  it("announces Chinese status label in ARIA live after task.state", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const store = useTaskCenterStore();
    store.handleBridgeEvent({
      kind: "task.state",
      event: {
        type: "task.state",
        taskId: "task-run-1" as TaskId,
        sessionId: "s" as SessionId,
        seq: 99,
        timestamp: new Date().toISOString(),
        payload: {
          taskId: "task-run-1" as TaskId,
          status: "waiting_permission",
          detail: null,
        },
      },
    });
    await nextTick();
    const live = wrapper.get('[data-testid="task-live-region"]');
    expect(live.attributes("aria-live")).toBe("polite");
    expect(live.text()).toMatch(/等待审批/);
    wrapper.unmount();
  });

  it("cancel confirmation failure shows alert without flipping status", async () => {
    const bridge = createBridge({ failCancel: true });
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    await wrapper.get('[data-task-id="task-run-1"] [data-testid="task-cancel"]').trigger("click");
    await nextTick();
    await wrapper.get('[data-testid="confirm-cancel"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="cancel-feedback"]').text()).toMatch(/取消失败/);
    expect(wrapper.get('[data-task-id="task-run-1"]').attributes("data-status")).toBe("running");
    wrapper.unmount();
  });

  it("card is not nested role=button; open control carries label", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const card = wrapper.get('[data-task-id="task-wait-1"]');
    expect(card.attributes("role")).toBeUndefined();
    expect(card.attributes("aria-current")).toBeUndefined();
    expect(card.attributes("aria-label")).toMatch(/等待审批/);
    expect(card.get('[data-testid="task-open"]').attributes("aria-label")).toMatch(/打开任务/);
    wrapper.unmount();
  });
});

describe("GAG-007 VirtualList", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("windows large lists and uses stable keys", async () => {
    const { h } = await import("vue");
    const items = Array.from({ length: 100 }, (_, i) => ({ id: `id-${i}`, n: i }));
    const wrapper = mount(VirtualList, {
      props: {
        items,
        itemHeight: 40,
        overscan: 2,
        getKey: (item: { id: string }) => item.id,
      },
      slots: {
        default: ({ item }: { item: { id: string } }) =>
          h("div", { class: "row", "data-id": item.id }, item.id),
      },
      attachTo: document.body,
    });
    await nextTick();
    const root = wrapper.get('[data-testid="virtual-list"]').element as HTMLElement;
    Object.defineProperty(root, "clientHeight", { configurable: true, value: 200 });
    // Trigger ResizeObserver path by re-reading — set internal via scroll event after height
    root.dispatchEvent(new Event("scroll"));
    await nextTick();

    const rows = wrapper.findAll(".virtual-list-row");
    // Default viewportHeight is 400 → ~10 visible + overscan*2; still << 100
    expect(rows.length).toBeLessThan(100);
    expect(rows.length).toBeGreaterThan(0);
    const spacer = wrapper.get('[data-testid="virtual-list-spacer"]');
    expect(spacer.attributes("style")).toMatch(/height:\s*4000px/);

    root.scrollTop = 800;
    root.dispatchEvent(new Event("scroll"));
    await nextTick();
    const after = wrapper.findAll("[data-id]");
    expect(after.length).toBeGreaterThan(0);
    const ids = after.map((n) => n.attributes("data-id") ?? "");
    expect(ids.some((id) => Number(id.replace("id-", "")) >= 15)).toBe(true);
    wrapper.unmount();
  });

  it("positions variable-height rows without leaving fixed-height gaps", async () => {
    const { h } = await import("vue");
    const items = [
      { id: "header", height: 36 },
      { id: "task", height: 120 },
      { id: "next-header", height: 36 },
    ];
    const wrapper = mount(VirtualList, {
      props: {
        items,
        itemHeight: 120,
        getItemHeight: (item: { height: number }) => item.height,
        getKey: (item: { id: string }) => item.id,
      },
      slots: {
        default: ({ item }: { item: { id: string } }) =>
          h("div", { "data-id": item.id }, item.id),
      },
      attachTo: document.body,
    });
    await nextTick();

    const rows = wrapper.findAll(".virtual-list-row");
    expect(rows).toHaveLength(3);
    expect(rows[0].attributes("style")).toMatch(/height:\s*36px/);
    expect(rows[1].attributes("style")).toMatch(/translateY\(36px\)/);
    expect(rows[2].attributes("style")).toMatch(/translateY\(156px\)/);
    expect(wrapper.get('[data-testid="virtual-list-spacer"]').attributes("style"))
      .toMatch(/height:\s*192px/);
    wrapper.unmount();
  });
});
