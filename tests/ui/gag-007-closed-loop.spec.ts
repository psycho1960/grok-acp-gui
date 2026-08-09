import { createPinia, setActivePinia } from "pinia";
import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import { createStatefulTaskCenterBridge } from "../../src/features/task-center/stateful-fake-bridge";
import { useTaskCenterStore } from "../../src/features/task-center/task-center-store";
import TaskCenterView from "../../src/features/task-center/TaskCenterView.vue";
import type { ProjectId } from "../../src/bridge/types";

describe("GAG-007 first-use closed loop", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    sessionStorage.clear();
  });

  it("shows project and new-task CTAs when no project is selected", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();
    expect(wrapper.find('[data-testid="project-empty"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="header-open-project"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="header-create-task"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="empty-open-project"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("routes the no-project new-task CTA to project selection", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();

    await wrapper.get('[data-testid="header-create-task"]').trigger("click");
    expect(wrapper.find('[data-testid="open-project-form"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="create-task-form"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("opens project, then shows create-task empty state", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);

    const opened = await store.openProjectPath("D:/work/demo-repo");
    expect(opened.ok).toBe(true);
    expect(store.hasActiveProject).toBe(true);

    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    // re-attach already done; view will attach again
    await Promise.resolve();
    await Promise.resolve();
    expect(wrapper.find('[data-testid="header-create-task"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="empty-create-task"]').exists()).toBe(true);
    expect(store.modelOptions).toContainEqual({ value: "grok-4.5", label: "grok-4.5", reasoningEffort: "high" });
    wrapper.unmount();
  });

  it("rejects missing directory", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const r = await store.openProjectPath("D:/missing/path");
    expect(r.ok).toBe(false);
    expect(r.code).toBe("invalid");
    expect(store.projectActionError).toMatch(/not exist|不存在|accessible/i);
  });

  it("accepts nongit path with notice code", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const r = await store.openProjectPath("D:/work/nongit-folder");
    expect(r.ok).toBe(true);
    expect(r.code).toBe("non_git");
  });

  it("creates task and returns taskId for conversation navigation", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);
    await store.openProjectPath("D:/work/demo-repo");
    const created = await store.createTask({
      prompt: "实现登录页\n包含表单校验",
      mode: "agent",
      workspaceStrategy: "worktree",
    });
    expect(created.ok).toBe(true);
    expect(created.taskId).toBeTruthy();
    expect(store.allTasks.some((t) => t.id === created.taskId)).toBe(true);
  });

  it("create fails without project", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const created = await store.createTask({ prompt: "x" });
    expect(created.ok).toBe(false);
    expect(created.message).toMatch(/项目/);
  });

  it("create fails on backend error path", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);
    await store.openProjectPath("D:/work/fail-create-repo");
    const created = await store.createTask({ prompt: "will fail" });
    expect(created.ok).toBe(false);
    expect(store.createTaskError).toMatch(/失败|error/i);
  });

  it("clearActiveProject removes selection", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const store = useTaskCenterStore();
    await store.attach(bridge);
    await store.openProjectPath("D:/work/demo-repo");
    expect(store.hasActiveProject).toBe(true);
    store.clearActiveProject();
    expect(store.hasActiveProject).toBe(false);
    expect(store.activeProjectId).toBeNull();
  });

  it("dialog flow: open project dialog submit creates active project", async () => {
    const bridge = createStatefulTaskCenterBridge();
    const wrapper = mount(TaskCenterView, {
      props: { bridge, syncHash: false },
      attachTo: document.body,
    });
    await flushPromises();

    await wrapper.get('[data-testid="header-open-project"]').trigger("click");
    expect(wrapper.find('[data-testid="open-project-form"]').exists()).toBe(true);

    const pathInput = wrapper.get('[data-testid="project-path-input"] input');
    await pathInput.setValue("D:/repos/my-app");
    const trust = wrapper.get('[data-testid="project-trust"]');
    // checkbox
    (trust.element as HTMLInputElement).checked = true;
    await trust.trigger("change");
    await wrapper.get('[data-testid="project-open-submit"]').trigger("click");
    await flushPromises();

    const store = useTaskCenterStore();
    expect(store.hasActiveProject).toBe(true);
    expect(wrapper.find('[data-testid="header-create-task"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("__setProjectsForTest forces no project empty state", async () => {
    const bridge = createStatefulTaskCenterBridge({
      initial: {
        projects: [
          {
            id: "p1" as ProjectId,
            path: "D:/a",
            displayPath: "a",
            lastOpenedAt: "2026-01-01T00:00:00.000Z",
          },
        ],
      },
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    expect(store.hasActiveProject).toBe(true);
    store.__setProjectsForTest([], null);
    expect(store.hasActiveProject).toBe(false);
  });
});
