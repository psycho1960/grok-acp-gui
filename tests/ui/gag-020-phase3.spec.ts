import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { h } from "vue";
import AppShell from "../../src/app/AppShell.vue";
import Drawer from "../../src/shared/ui/Drawer.vue";
import Button from "../../src/shared/ui/Button.vue";

describe("GAG-020 phase 3 polish", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("AppShell exposes skip link and resizer keyboard hint copy", async () => {
    const wrapper = mount(AppShell, {
      props: {
        left: h("div", "left"),
        main: h("div", "main"),
        inspector: h("div", "inspector"),
        inspectorOpen: true,
      },
      attachTo: document.body,
    });
    await flushPromises();
    const skip = wrapper.get('[data-testid="skip-to-content"]');
    expect(skip.attributes("href")).toBe("#main-content");
    expect(wrapper.find("#main-content").exists()).toBe(true);
    // On wide viewports left resizer is present with keyboard guidance.
    const left = wrapper.find('[data-testid="left-resizer"]');
    if (left.exists()) {
      expect(left.attributes("aria-label") ?? "").toMatch(/方向键|键盘|调整/);
    }
    wrapper.unmount();
  });

  it("Button loading state uses SVG spinner instead of hourglass glyph", () => {
    const wrapper = mount(Button, {
      props: { state: "loading" },
      slots: { default: () => "保存" },
    });
    expect(wrapper.html()).not.toContain("⌛");
    expect(wrapper.find(".spinner").exists()).toBe(true);
    wrapper.unmount();
  });

  it("Drawer closes after a rightward swipe gesture", async () => {
    const wrapper = mount(Drawer, {
      props: { modelValue: true, title: "面板" },
      attachTo: document.body,
    });
    const panel = wrapper.get(".drawer");
    await panel.trigger("pointerdown", { clientX: 40, pointerType: "touch", button: 0 });
    await panel.trigger("pointerup", { clientX: 140, pointerType: "touch", button: 0 });
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual([false]);
    wrapper.unmount();
  });
});
