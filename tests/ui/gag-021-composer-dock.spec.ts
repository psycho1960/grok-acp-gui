import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import Composer from "../../src/features/conversation/Composer.vue";
import { FIX_TASK, fixtureSessionSnapshot } from "../../src/features/conversation/fixtures";
import type { ModelInfo } from "../../src/bridge/types";

const MODELS: ModelInfo[] = [
  { modelId: "grok-4.5", name: "grok-4.5" },
  { modelId: "deepseek", name: "deepseek" },
];

describe("GAG-021 composer dock", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("idle dock has one send circle that stays inert without content", () => {
    const w = mount(Composer, {
      props: {
        modelValue: "",
        capabilities: { canSend: true, canCancel: false, bridgeOnline: true },
      },
    });
    expect(w.find('[data-testid="composer-send"]').exists()).toBe(true);
    expect(w.find('[data-testid="composer-stop"]').exists()).toBe(false);
    expect(
      (w.get('[data-testid="composer-send"]').element as HTMLButtonElement).disabled,
    ).toBe(true);
    w.unmount();
  });

  it("running dock shows only Stop, not a second Send", () => {
    const w = mount(Composer, {
      props: {
        modelValue: "follow up",
        capabilities: { canSend: false, canCancel: true, bridgeOnline: true },
      },
    });
    expect(w.find('[data-testid="composer-stop"]').exists()).toBe(true);
    expect(w.find('[data-testid="composer-send"]').exists()).toBe(false);
    w.unmount();
  });

  it("Shift+Enter does not send", async () => {
    const w = mount(Composer, {
      props: {
        modelValue: "hi",
        capabilities: { canSend: true, canCancel: false, bridgeOnline: true },
      },
    });
    await w.get('[data-testid="composer-input"]').trigger("keydown", {
      key: "Enter",
      shiftKey: true,
    });
    expect(w.emitted("send")).toBeFalsy();
    w.unmount();
  });

  it("keeps model and reasoning in one dock control and drops the chevron when locked", async () => {
    const idle = mount(Composer, {
      props: {
        modelValue: "",
        capabilities: { canSend: true, canCancel: false, bridgeOnline: true },
        models: MODELS,
        selectedModel: "grok-4.5",
        selectedReasoning: "high",
        settingsLocked: false,
      },
    });
    expect(idle.get('[data-testid="composer-model-control"]').text()).toMatch(/grok-4\.5/);
    expect(idle.get('[data-testid="composer-model-control"]').text()).toMatch(/高/);
    expect(idle.find('[data-testid="model-chevron"]').exists()).toBe(true);
    expect(idle.find('[data-testid="conversation-model-select"] select').exists()).toBe(true);
    idle.unmount();

    const locked = mount(Composer, {
      props: {
        modelValue: "",
        capabilities: { canSend: false, canCancel: true, bridgeOnline: true },
        models: MODELS,
        selectedModel: "grok-4.5",
        selectedReasoning: "high",
        settingsLocked: true,
      },
    });
    expect(locked.find('[data-testid="model-chevron"]').exists()).toBe(false);
    locked.unmount();
  });

  it("slash button opens the same command list as typing /", async () => {
    const commands = [{ name: "init", description: "初始化" }];
    const typed = mount(Composer, {
      props: {
        modelValue: "/",
        capabilities: { canSend: true, canCancel: false, bridgeOnline: true },
        slashCommands: commands,
      },
      attachTo: document.body,
    });
    const input = typed.get('[data-testid="composer-input"]');
    (input.element as HTMLTextAreaElement).setSelectionRange(1, 1);
    await input.trigger("keyup");
    const typedNames = typed.findAll('[data-testid="slash-menu-item"]').map((item) => item.text());
    typed.unmount();

    const clicked = mount(Composer, {
      props: {
        modelValue: "",
        capabilities: { canSend: true, canCancel: false, bridgeOnline: true },
        slashCommands: commands,
      },
    });
    await clicked.get('[data-testid="composer-slash-help"]').trigger("click");
    const clickedNames = clicked.findAll('[data-testid="slash-menu-item"]').map((item) => item.text());
    expect(clickedNames).toEqual(typedNames);
    expect(clickedNames[0]).toContain("init");
    clicked.unmount();
  });

  it("removes the header Stop duplicate and header model selects", async () => {
    const w = mount(ConversationView, {
      props: {
        bridge: createFakeDesktopBridge(),
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot({ status: "running" }),
      },
      attachTo: document.body,
    });
    await flushPromises();
    expect(w.find('[data-testid="header-stop"]').exists()).toBe(false);
    expect(w.find('[data-testid="conversation-header"] [data-testid="conversation-model-select"]').exists()).toBe(false);
    expect(w.find('[data-testid="composer"] [data-testid="conversation-model-select"]').exists()).toBe(true);
    w.unmount();
  });
});
