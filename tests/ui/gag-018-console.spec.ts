import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { nextTick } from "vue";
import ShellView from "../../src/app/ShellView.vue";
import TaskCenterView from "../../src/features/task-center/TaskCenterView.vue";
import { createTaskCenterSeedSnapshot } from "../../src/features/task-center/seed";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopCommand } from "../../src/bridge/types";
import Composer from "../../src/features/conversation/Composer.vue";

function seedBridge() {
  const snapshot = createTaskCenterSeedSnapshot();
  return createFakeDesktopBridge({
    bootstrapSnapshot: snapshot,
    onExecute(command: DesktopCommand) {
      if (command.type === "project.list" || command.type === "task.list") {
        return { success: "true", data: snapshot };
      }
      return { success: "true", data: {} };
    },
  });
}

describe("GAG-018 console density", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    window.location.hash = "";
  });

  it("collapses TaskCenter filters until toggled", async () => {
    const wrapper = mount(TaskCenterView, {
      props: { bridge: seedBridge(), syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    expect(wrapper.find('[data-testid="task-filters-panel"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="task-search"]').exists()).toBe(true);
    await wrapper.get('[data-testid="toggle-filters"]').trigger("click");
    await nextTick();
    expect(wrapper.find('[data-testid="task-filters-panel"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="task-filter-status"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("ShellView exposes breadcrumb and status bar regions", async () => {
    window.location.hash = "#task-center";
    const wrapper = mount(ShellView, {
      global: { plugins: [createPinia()] },
      attachTo: document.body,
    });
    await flushPromises();
    expect(wrapper.find('[data-testid="topbar-breadcrumb"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="status-bar"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="nav-all-tasks"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="group-chip-running"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("Composer uses short placeholder and slash help button", async () => {
    const wrapper = mount(Composer, {
      props: {
        modelValue: "",
        capabilities: { canSend: true, canCancel: false, bridgeOnline: true },
        slashCommands: [{ name: "plan", description: "计划" }],
      },
    });
    const input = wrapper.get('[data-testid="composer-input"]');
    expect((input.element as HTMLTextAreaElement).placeholder).toBe("输入消息…");
    await wrapper.get('[data-testid="composer-slash-help"]').trigger("click");
    await nextTick();
    expect(wrapper.find('[data-testid="slash-menu"]').exists()).toBe(true);
    wrapper.unmount();
  });
});
