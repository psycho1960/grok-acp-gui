import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import ShellView from "../../src/app/ShellView.vue";
import ErrorState from "../../src/shared/ui/ErrorState.vue";
import { mapErrorMessage } from "../../src/shared/ui/error-map";
import CommandPalette, { type CommandItem } from "../../src/shared/ui/CommandPalette.vue";

describe("GAG-019 error map", () => {
  it("maps ACP handshake failures to friendly copy", () => {
    const mapped = mapErrorMessage("ACP handshake failed: EOF");
    expect(mapped.title).toContain("Grok");
    expect(mapped.suggestion).toBeTruthy();
    expect(mapped.raw).toContain("handshake");
  });

  it("ErrorState exposes copy detail action", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText },
    });
    const wrapper = mount(ErrorState, {
      props: { detail: "ACP handshake failed: EOF" },
    });
    expect(wrapper.text()).toContain("无法与 Grok");
    await wrapper.get('[data-testid="error-copy-detail"]').trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalled();
    wrapper.unmount();
    vi.unstubAllGlobals();
  });
});

describe("GAG-019 command palette", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    window.location.hash = "";
  });

  it("filters and runs a command", async () => {
    const run = vi.fn();
    const items: CommandItem[] = [
      {
        id: "tasks",
        label: "打开任务中心",
        group: "页面",
        icon: "list",
        run,
      },
      {
        id: "recovery",
        label: "打开恢复中心",
        group: "页面",
        icon: "shield",
        run: vi.fn(),
      },
    ];
    const wrapper = mount(CommandPalette, {
      props: { modelValue: true, items },
      attachTo: document.body,
    });
    await nextTick();
    const input = wrapper.get('[data-testid="command-palette-input"]');
    await input.setValue("恢复");
    await nextTick();
    expect(wrapper.find('[data-testid="command-item-tasks"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="command-item-recovery"]').exists()).toBe(true);
    await wrapper.get('[data-testid="command-item-recovery"]').trigger("click");
    expect(wrapper.emitted("update:modelValue")?.[0]).toEqual([false]);
    wrapper.unmount();
  });

  it("ShellView opens command palette via control", async () => {
    window.location.hash = "#task-center";
    const wrapper = mount(ShellView, {
      global: { plugins: [createPinia()] },
      attachTo: document.body,
    });
    await flushPromises();
    await wrapper.get('[data-testid="open-command-palette"]').trigger("click");
    await nextTick();
    expect(wrapper.find('[data-testid="command-palette"]').exists()).toBe(true);
    wrapper.unmount();
  });
});
