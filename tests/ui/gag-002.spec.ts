import { mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref, type VNode } from "vue";
import { afterEach, describe, expect, it } from "vitest";
import AppShell from "../../src/app/AppShell.vue";
import Button from "../../src/shared/ui/Button.vue";
import Dialog from "../../src/shared/ui/Dialog.vue";
import Drawer from "../../src/shared/ui/Drawer.vue";
import IconButton from "../../src/shared/ui/IconButton.vue";
import Tooltip from "../../src/shared/ui/Tooltip.vue";

type Viewport = { width: number; dppx?: number };

function installViewport({ width, dppx = 1 }: Viewport): void {
  window.matchMedia = ((query: string) => ({
    matches: query.includes("1200") ? width <= 1200 || dppx >= 1.75 : query.includes("1080") ? width <= 1080 : width <= 1023 || dppx >= 1.75,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

function slot(label: string): VNode { return h("button", { type: "button" }, label); }

function mountShell(viewport: Viewport, inspectorOpen = true) {
  installViewport(viewport);
  return mount(AppShell, {
    attachTo: document.body,
    props: { left: slot("Left task"), main: slot("Main composer"), inspector: slot("Inspector action"), inspectorOpen, statusBar: slot("Status") },
  });
}

function mountOverlay(component: typeof Dialog | typeof Drawer, title: string) {
  const open = ref(false);
  const isDialog = component === Dialog;
  const Host = defineComponent({
    setup() {
      return () => h("div", [
        h("button", { type: "button", "data-testid": "trigger", onClick: () => { open.value = true; } }, "Trigger"),
        h(component, { modelValue: open.value, title, "onUpdate:modelValue": (value: boolean) => { open.value = value; } }, {
          default: () => isDialog
            ? h("button", { type: "button", "data-testid": "first-action" }, "First action")
            : [h("button", { type: "button", "data-testid": "first-action" }, "First action"), h("button", { type: "button", "data-testid": "last-action" }, "Last action")],
          actions: () => isDialog ? h("button", { type: "button", "data-testid": "last-action" }, "Last action") : undefined,
        }),
      ]);
    },
  });
  return mount(Host, { attachTo: document.body });
}

afterEach(() => { document.body.innerHTML = ""; });

describe("GAG-002 accessible controls", () => {
  it("renders Button state and prevents loading activation", async () => {
    const wrapper = mount(Button, { props: { state: "loading" }, slots: { default: "Save" } });
    expect(wrapper.attributes("data-state")).toBe("loading");
    expect(wrapper.attributes("aria-busy")).toBe("true");
    expect(wrapper.attributes("disabled")).toBeDefined();
    await wrapper.trigger("click");
    expect(wrapper.emitted("click")).toBeUndefined();
  });

  it("links Tooltip content to its trigger", () => {
    const wrapper = mount(Tooltip, { props: { text: "Helpful text" }, slots: { default: h(IconButton, { label: "Info" }, { default: () => "i" }) } });
    const trigger = wrapper.get('[aria-label="Info"]');
    const descriptionId = trigger.attributes("aria-describedby");
    expect(descriptionId).toBeTruthy();
    expect(wrapper.get(`#${descriptionId}`).text()).toBe("Helpful text");
  });

  for (const [name, component, closeLabel] of [["Dialog", Dialog, "关闭对话框"], ["Drawer", Drawer, "关闭抽屉"]] as const) {
    it(`${name} traps Tab, closes with Escape, and restores focus`, async () => {
      const wrapper = mountOverlay(component, `${name} title`);
      const trigger = wrapper.get('[data-testid="trigger"]');
      trigger.element.focus();
      await trigger.trigger("click");
      await nextTick();
      await nextTick();
      const first = wrapper.get(`[aria-label="${closeLabel}"]`);
      const last = wrapper.get('[data-testid="last-action"]');
      expect(document.activeElement).toBe(first.element);
      await last.trigger("keydown", { key: "Tab" });
      expect(document.activeElement).toBe(first.element);
      await first.trigger("keydown", { key: "Tab", shiftKey: true });
      expect(document.activeElement).toBe(last.element);
      await last.trigger("keydown", { key: "Escape" });
      await nextTick();
      expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
      expect(document.activeElement).toBe(trigger.element);
    });
  }
});

describe("GAG-002 AppShell responsive behavior", () => {
  it("adjusts both side panels by keyboard and removes the Inspector column when collapsed", async () => {
    const wrapper = mountShell({ width: 1440 });
    const left = wrapper.get('[aria-label="调整左侧栏宽度"]');
    const right = wrapper.get('[aria-label="调整 Inspector 宽度"]');
    await left.trigger("keydown", { key: "ArrowRight" });
    await right.trigger("keydown", { key: "ArrowLeft" });
    expect(left.attributes("aria-valuenow")).toBe("272");
    expect(right.attributes("aria-valuenow")).toBe("392");
    await wrapper.get('[aria-label="打开 Inspector"]').trigger("click");
    expect(wrapper.emitted("update:inspectorOpen")?.[0]).toEqual([false]);
    await wrapper.setProps({ inspectorOpen: false });
    expect(wrapper.find(".shell-columns").classes()).not.toContain("has-inspector");
    expect(wrapper.find('[aria-label="调整 Inspector 宽度"]').exists()).toBe(false);
  });

  for (const viewport of [{ width: 1440 }, { width: 1200 }, { width: 1024 }, { width: 1440, dppx: 2 }]) {
    it(`keeps the required actions reachable at ${viewport.width}px${viewport.dppx ? " / 200%" : ""}`, async () => {
      const wrapper = mountShell(viewport);
      await nextTick();
      const summary = {
        hasLeftPanel: wrapper.find(".shell-left").exists(),
        hasInspectorPanel: wrapper.find(".shell-inspector").exists(),
        hasNavigationToggle: wrapper.find('[aria-label="打开任务导航"]').exists(),
        hasInspectorToggle: wrapper.find('[aria-label="打开 Inspector"]').exists(),
      };
      expect(summary.hasInspectorToggle).toBe(true);
      if (viewport.dppx) {
        expect(summary.hasLeftPanel).toBe(false);
        expect(summary.hasNavigationToggle).toBe(true);
        await wrapper.get('[aria-label="打开任务导航"]').trigger("click");
        expect(wrapper.get('[role="dialog"][aria-label="任务导航"]')).toBeTruthy();
      }
      if (viewport.width === 1200) expect(summary.hasInspectorPanel).toBe(false);
      if (viewport.width === 1024) {
        expect(wrapper.find('[aria-label="调整左侧栏宽度"]').exists()).toBe(false);
        expect(wrapper.attributes("style")).toContain("--left-width: 220px");
      }
      expect(summary).toMatchSnapshot();
    });
  }

});
