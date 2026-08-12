import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopCommand, FileChange, TaskId } from "../../src/bridge/types";
import ReviewView from "../../src/features/review/ReviewView.vue";

const taskId = "GAG-012-task" as TaskId;
const files: FileChange[] = [
  {
    path: "src/alpha.ts",
    kind: "modified",
    binary: false,
    size: 42,
    mode: "file",
    fingerprint: "fp-alpha",
    staged: false,
    conflicted: false,
    submodule: false,
  },
  {
    path: "assets/image.png",
    kind: "untracked",
    binary: true,
    size: 2048,
    mode: "file",
    fingerprint: "fp-image",
    staged: false,
    conflicted: false,
    submodule: false,
  },
  {
    path: "vendor/module",
    kind: "modified",
    binary: false,
    size: 0,
    mode: "submodule",
    fingerprint: "fp-module",
    staged: false,
    conflicted: false,
    submodule: true,
  },
];

function bridge(commands: DesktopCommand[], checkpointFails = false) {
  return createFakeDesktopBridge({
    onExecute(command) {
      commands.push(command);
      if (command.type === "review.status") {
        return { success: "true", data: { snapshot: { head: "a".repeat(40), version: "v1", files } } };
      }
      if (command.type === "review.checkpoints") {
        return { success: "true", data: { checkpoints: [] } };
      }
      if (command.type === "review.diff") {
        const file = files.find((item) => item.path === command.payload.path)!;
        return {
          success: "true",
          data: { document: { path: file.path, binary: file.binary, oversized: false, truncated: false, text: file.binary ? undefined : "@@ -1 +1 @@\n-old\n+new", bytes: file.size } },
        };
      }
      if (command.type === "review.validate") {
        return { success: "true", data: { validation: { valid: true, stalePaths: [], missingPaths: [] } } };
      }
      if (command.type === "review.checkpoint") {
        if (checkpointFails) {
          return { success: "false", error: { code: "GIT_SELECTION_STALE", message: "选择已过期", retryable: false, detailsRedacted: true, correlationId: "test" } };
        }
        return {
          success: "true",
          data: { receipt: { id: "cp-1", taskId, attemptNumber: 1, commitSha: "b".repeat(40), treeSha: "c".repeat(40), headBefore: "a".repeat(40), selectionManifest: command.payload.selection, selectionHash: "hash", message: command.payload.message, createdAt: "2026-08-09T00:00:00Z", remainingFiles: [files[1]] } },
        };
      }
      return { success: "true", data: {} };
    },
  });
}

describe("GAG-012 ReviewView", () => {
  it("shows text/binary states and prevents unsupported selections", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(ReviewView, { props: { bridge: bridge(commands), taskId } });
    await flushPromises();
    expect(wrapper.text()).toContain("src/alpha.ts");
    expect(wrapper.text()).toContain("子模块");
    const checkboxes = wrapper.findAll('input[type="checkbox"]');
    expect(checkboxes[2].attributes("disabled")).toBeDefined();
    await wrapper.findAll(".files button")[1].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("二进制文件 · 2048 字节");
    await wrapper.get('[data-testid="review-file-search"] input').setValue("alpha");
    expect(wrapper.findAll(".files li")).toHaveLength(1);
  });

  it("binds fingerprints and preserves selection/message after checkpoint failure", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(ReviewView, { props: { bridge: bridge(commands, true), taskId } });
    await flushPromises();
    await wrapper.findAll('input[type="checkbox"]')[0].setValue(true);
    await wrapper.get("textarea").setValue("fix(GAG-012): selected fix [GAG-012]");
    await wrapper.findAll("button").find((button) => button.text().includes("创建 Checkpoint"))!.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("选择已过期");
    expect((wrapper.get('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(true);
    expect((wrapper.get("textarea").element as HTMLTextAreaElement).value).toBe("fix(GAG-012): selected fix [GAG-012]");
    expect(commands).toContainEqual({
      type: "review.validate",
      payload: { taskId, selection: [{ path: "src/alpha.ts", fingerprint: "fp-alpha" }] },
    });
  });

  it("styles checkpoint textareas under .checkpoint (not .files)", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(ReviewView, { props: { bridge: bridge(commands), taskId } });
    await flushPromises();
    const textareas = wrapper.findAll("textarea");
    expect(textareas.length).toBeGreaterThanOrEqual(2);
    for (const ta of textareas) {
      expect(ta.element.closest(".checkpoint")).not.toBeNull();
      expect(ta.element.closest(".files")).toBeNull();
      expect(ta.element.matches(".checkpoint textarea")).toBe(true);
    }
  });
});
