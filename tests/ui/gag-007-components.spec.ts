import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { nextTick } from "vue";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { TaskOpenResult } from "../../src/bridge/types";
import TaskCenterView from "../../src/features/task-center/TaskCenterView.vue";
import { createTaskCenterSeedSnapshot } from "../../src/features/task-center/seed";

function createBridge() {
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
    expect(wrapper.find('[data-testid="task-list"]').exists()).toBe(true);
    expect(wrapper.findAll("[data-task-id]").length).toBeGreaterThan(0);
    wrapper.unmount();
  });

  it("shows empty state when bootstrap has no tasks", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: {
        ...createTaskCenterSeedSnapshot(),
        activeTasks: [],
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

  it("opens detail drawer from card click and keyboard", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const card = wrapper.get('[data-task-id="task-run-1"]');
    await card.trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="task-detail"]').exists()).toBe(true);
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("实现 Task Center UI");

    // Close via drawer close button restores focus path is covered by Drawer tests;
    // ensure Escape closes.
    await wrapper.get('[aria-label="关闭抽屉"]').trigger("keydown", { key: "Escape" });
    await nextTick();
    await nextTick();
    expect(wrapper.find('[data-testid="task-detail"]').exists()).toBe(false);
    wrapper.unmount();
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

  it("exposes ARIA live region for status updates", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const live = wrapper.get(".sr-live");
    expect(live.attributes("aria-live")).toBe("polite");
    wrapper.unmount();
  });

  it("opens cancel confirmation without flipping status", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const cancel = wrapper
      .findAll('[data-testid="task-cancel"]')
      .find((n) => n.isVisible());
    expect(cancel).toBeTruthy();
    await cancel!.trigger("click");
    await nextTick();
    expect(wrapper.text()).toContain("确认取消任务");
    expect(wrapper.get('[data-task-id="task-run-1"]').attributes("data-status")).toBe(
      "running",
    );
    wrapper.unmount();
  });

  it("marks card aria-label with status text (color not sole signal)", async () => {
    const bridge = createBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    const card = wrapper.get('[data-task-id="task-wait-1"]');
    expect(card.attributes("aria-label")).toMatch(/等待审批/);
    wrapper.unmount();
  });
});
