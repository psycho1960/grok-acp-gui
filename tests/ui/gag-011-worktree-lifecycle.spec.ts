import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopCommand, TaskId, WorktreeRecord } from "../../src/bridge/types";
import WorktreePanel from "../../src/features/worktrees/WorktreePanel.vue";

const taskId = "task-gag-011" as TaskId;
const path = "D:\\managed\\repo\\task-gag-011";

function record(state: WorktreeRecord["state"] = "dirty"): WorktreeRecord {
  return {
    id: "wt-gag-011",
    taskId,
    repoRoot: "D:\\repo",
    path,
    displayPath: "repo/task-gag-011",
    branch: "gag/task-gag-011-worktree",
    baseBranch: "main",
    baseCommit: "0123456789abcdef",
    ownership: "managed",
    state,
    diskUsageBytes: 2048,
    lastVerifiedAt: "2026-08-09T08:00:00Z",
    locked: false,
  };
}

describe("GAG-011 WorktreePanel", () => {
  it("requires exact absolute-path confirmation and passes backend token unchanged", async () => {
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        commands.push(command);
        if (command.type === "worktree.inspect") {
          return { success: "true", data: { worktree: record() } };
        }
        if (command.type === "worktree.prepareRemoval") {
          return {
            success: "true",
            data: {
              preparation: {
                confirmationToken: "opaque-confirmation-token",
                absolutePath: path,
                dirty: true,
                untrackedFiles: 2,
                forceRequired: true,
                recovery: {
                  id: "recovery-1",
                  manifestPath: "D:\\recovery\\manifest.json",
                  branchBundle: "D:\\recovery\\branch.bundle",
                  trackedPatch: "D:\\recovery\\tracked.patch",
                  untrackedZip: "D:\\recovery\\untracked.zip",
                },
              },
            },
          };
        }
        if (command.type === "worktree.remove") {
          return { success: "true", data: { worktree: record("removed") } };
        }
        return { success: "true", data: {} };
      },
    });
    const wrapper = mount(WorktreePanel, { props: { bridge, taskId } });
    await flushPromises();
    expect(wrapper.text()).toContain("有未提交内容");
    expect(wrapper.text()).toContain("2.0 KiB");

    await wrapper.get('[data-testid="worktree-prepare-removal"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain(path);
    expect(wrapper.text()).toContain("恢复包已创建并验证");
    const confirm = wrapper.get('[data-testid="worktree-confirm-removal"]');
    expect(confirm.attributes("disabled")).toBeDefined();

    const pathInput = wrapper.get('[data-testid="worktree-confirm-path"] input');
    // Force cleanup uses DELETE keyword in the UI; backend still receives absolute path.
    await pathInput.setValue("WRONG");
    expect(confirm.attributes("disabled")).toBeDefined();
    await pathInput.setValue("DELETE");
    expect(confirm.attributes("disabled")).toBeDefined();
    await wrapper.get('input[type="checkbox"]').setValue(true);
    expect(confirm.attributes("disabled")).toBeUndefined();
    await confirm.trigger("click");
    await flushPromises();

    expect(commands.at(-1)).toEqual({
      type: "worktree.remove",
      payload: {
        taskId,
        confirmationToken: "opaque-confirmation-token",
        confirmedPath: path,
      },
    });
    expect(wrapper.text()).toContain("removed");
  });

  it("shows missing honestly and offers no destructive action", async () => {
    const execute = vi.fn(() => ({
      success: "true" as const,
      data: { worktree: record("missing") },
    }));
    const bridge = createFakeDesktopBridge({ onExecute: execute });
    const wrapper = mount(WorktreePanel, { props: { bridge, taskId } });
    await flushPromises();
    expect(wrapper.text()).toContain("missing");
    expect(wrapper.find('[data-testid="worktree-prepare-removal"]').exists()).toBe(false);
  });

  it("requires a prepared token and exact path before adopting an external worktree", async () => {
    const externalPath = "D:\\external\\linked";
    const commands: DesktopCommand[] = [];
    const bridge = createFakeDesktopBridge({
      onExecute(command) {
        commands.push(command);
        if (command.type === "worktree.inspect") {
          return { success: "false", error: { code: "WORKTREE_MISSING", message: "missing" } };
        }
        if (command.type === "worktree.reconcile") {
          return {
            success: "true",
            data: {
              worktrees: [
                {
                  ...record("ready"),
                  id: "external-1",
                  taskId: "external-1" as TaskId,
                  path: externalPath,
                  ownership: "external",
                },
              ],
            },
          };
        }
        if (command.type === "worktree.prepareAdoption") {
          return {
            success: "true",
            data: {
              preparation: {
                confirmationToken: "adopt-token",
                absolutePath: externalPath,
              },
            },
          };
        }
        if (command.type === "worktree.adopt") {
          return { success: "true", data: { worktree: { ...record("ready"), ownership: "adopted" } } };
        }
        return { success: "true", data: {} };
      },
    });
    const wrapper = mount(WorktreePanel, { props: { bridge, taskId } });
    await flushPromises();
    await wrapper.get("button:nth-of-type(2)").trigger("click");
    await flushPromises();
    await wrapper.get(".external-list button").trigger("click");
    await flushPromises();
    const confirm = wrapper.get('[data-testid="worktree-confirm-adoption"]');
    expect(confirm.attributes("disabled")).toBeDefined();
    await wrapper
      .get('[data-testid="worktree-confirm-adoption-path"] input')
      .setValue(externalPath);
    expect(confirm.attributes("disabled")).toBeUndefined();
    await confirm.trigger("click");
    await flushPromises();
    expect(commands.at(-1)).toEqual({
      type: "worktree.adopt",
      payload: {
        taskId,
        path: externalPath,
        confirmationToken: "adopt-token",
        confirmedPath: externalPath,
      },
    });
  });
});
