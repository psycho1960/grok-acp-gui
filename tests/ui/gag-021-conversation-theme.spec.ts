import { createPinia, setActivePinia } from "pinia";
import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import ConversationView from "../../src/features/conversation/ConversationView.vue";
import { FIX_TASK, fixtureSessionSnapshot } from "../../src/features/conversation/fixtures";
import TaskCenterView from "../../src/features/task-center/TaskCenterView.vue";
import { createTaskCenterSeedSnapshot } from "../../src/features/task-center/seed";
import { applyThemeTokens } from "../../src/shared/theme/tokens";

function tokenOn(el: Element, name: string): string {
  let node: HTMLElement | null = el as HTMLElement;
  while (node) {
    const value = node.style.getPropertyValue(name).trim();
    if (value) return value.toLowerCase();
    node = node.parentElement;
  }
  return document.documentElement.style.getPropertyValue(name).trim().toLowerCase();
}

describe("GAG-021 conversation Rose Pine Moon", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
    applyThemeTokens(document.documentElement.style);
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.documentElement.removeAttribute("style");
  });

  it("conversation canvas, task bar, composer, and rail use Rose Pine Moon, not Mocha gray", async () => {
    const w = mount(ConversationView, {
      props: {
        bridge: createFakeDesktopBridge(),
        taskId: FIX_TASK,
        snapshot: fixtureSessionSnapshot(),
      },
      attachTo: document.body,
    });
    await Promise.resolve();
    await Promise.resolve();

    const canvas = w.get('[data-testid="conversation-view"]').element;
    const header = w.get('[data-testid="conversation-header"]').element;
    const composer = w.get('[data-testid="composer"]').element;
    const rail = w.get('[data-testid="artifact-panel"]').element;

    for (const surface of [canvas, header, composer, rail]) {
      expect(tokenOn(surface, "--ctp-base")).toBe("#232136");
      expect(tokenOn(surface, "--ctp-mantle")).toBe("#2a273f");
      expect(tokenOn(surface, "--ctp-text")).toBe("#e0def4");
      expect(tokenOn(surface, "--ctp-mauve")).toBe("#c4a7e7");
      expect(tokenOn(surface, "--ctp-red")).toBe("#eb6f92");
      expect(tokenOn(surface, "--ctp-blue")).toBe("#9ccfd8");
    }

    w.unmount();
  });

  it("task center stays on Mocha tokens", async () => {
    const w = mount(TaskCenterView, {
      props: {
        bridge: createFakeDesktopBridge({
          bootstrapSnapshot: createTaskCenterSeedSnapshot(),
        }),
        syncHash: false,
      },
      attachTo: document.body,
    });
    await Promise.resolve();
    await Promise.resolve();

    const center = w.get('[data-testid="task-center"]').element;
    expect(tokenOn(center, "--ctp-base")).toBe("#1e1e2e");
    expect(tokenOn(center, "--ctp-mantle")).toBe("#181825");
    expect(tokenOn(center, "--ctp-mauve")).toBe("#cba6f7");

    w.unmount();
  });
});
