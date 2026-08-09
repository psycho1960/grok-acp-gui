// GAG-004 P1 regression: when the database is unavailable, `bootstrap()`
// returns `{ ready: false, dbError: "..." }`. The Renderer MUST render
// `ErrorState` (UI-ERROR-001) and MUST NOT render `ShellView`.
//
// Before the fix, `App.vue` only checked the `await bootstrap()` promise
// for rejection; the promise resolved normally even when `ready=false`,
// so `ShellView` rendered with no underlying persistence.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import type { BootstrapSnapshot } from "../../src/bridge/types";

// Stub ShellView so we don't pull AppShell/matchMedia into this test.
// eslint-disable-next-line vue/one-component-per-file
const ShellViewStub = defineComponent({
  name: "ShellView",
  setup() {
    return () => h("div", { "data-testid": "shell-view-stub" });
  },
});

// eslint-disable-next-line vue/one-component-per-file
const OnboardingViewStub = defineComponent({
  name: "OnboardingView",
  emits: ["ready"],
  setup(_, { emit }) {
    return () =>
      h(
        "button",
        { "data-testid": "onboarding-view-stub", onClick: () => emit("ready", {}) },
        "startup checks",
      );
  },
});

// `vi.mock` factories are hoisted to the top of the file; any variable they
// reference must also be hoisted via `vi.hoisted` to avoid "Cannot access X
// before initialization" ReferenceErrors.
const { mockBootstrap } = vi.hoisted(() => ({ mockBootstrap: vi.fn() }));

vi.mock("../../src/bridge/desktop-bridge", () => ({
  bootstrap: mockBootstrap,
}));

import App from "../../src/App.vue";

function readySnapshot(): BootstrapSnapshot {
  return {
    productName: "Grok ACP GUI",
    version: "0.1.16",
    platform: "win32",
    ready: true,
    runtime: { status: "ready" },
    capabilities: { models: [], modes: [], slashCommands: [] },
    projects: [],
    activeTasks: [],
    bindings: [],
    worktrees: [],
    recoveryItems: [],
    settings: [],
    recoveryPerformed: false,
    tasksInterrupted: 0,
  };
}

function notReadySnapshot(dbError: string): BootstrapSnapshot {
  return {
    ...readySnapshot(),
    ready: false,
    dbError,
    runtime: { status: "unavailable" },
  };
}

describe("App startup error gate (UI-ERROR-001)", () => {
  beforeEach(() => {
    mockBootstrap.mockReset();
  });

  it("renders ErrorState and NOT ShellView when ready=false with dbError", async () => {
    mockBootstrap.mockResolvedValue(
      notReadySnapshot("Database unavailable (DB_MIGRATION_FAILED)."),
    );

    const wrapper = mount(App, {
      attachTo: document.body,
      global: { stubs: { ShellView: ShellViewStub } },
    });
    await flushPromises();

    expect(wrapper.find('[data-err="UI-ERROR-001"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="shell-view-stub"]').exists()).toBe(false);
  });

  it("renders ShellView when ready=true and no dbError", async () => {
    mockBootstrap.mockResolvedValue(readySnapshot());

    const wrapper = mount(App, {
      attachTo: document.body,
      global: { stubs: { ShellView: ShellViewStub } },
    });
    await flushPromises();

    expect(wrapper.find('[data-err="UI-ERROR-001"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="shell-view-stub"]').exists()).toBe(true);
  });

  it("renders ErrorState when ready=false even without dbError string", async () => {
    // Defensive: ready=false alone must block ShellView.
    mockBootstrap.mockResolvedValue({ ...readySnapshot(), ready: false });

    const wrapper = mount(App, {
      attachTo: document.body,
      global: { stubs: { ShellView: ShellViewStub } },
    });
    await flushPromises();

    expect(wrapper.find('[data-err="UI-ERROR-001"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="shell-view-stub"]').exists()).toBe(false);
  });

  it("gates the shell on real runtime readiness and continues after checks pass", async () => {
    mockBootstrap.mockResolvedValue({
      ...readySnapshot(),
      runtime: { status: "probing", version: "1.0.0" },
    });
    const wrapper = mount(App, {
      attachTo: document.body,
      global: {
        stubs: { ShellView: ShellViewStub, OnboardingView: OnboardingViewStub },
      },
    });
    await flushPromises();

    expect(wrapper.find('[data-testid="onboarding-view-stub"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="shell-view-stub"]').exists()).toBe(false);
    await wrapper.get('[data-testid="onboarding-view-stub"]').trigger("click");
    expect(wrapper.find('[data-testid="shell-view-stub"]').exists()).toBe(true);
  });
});
