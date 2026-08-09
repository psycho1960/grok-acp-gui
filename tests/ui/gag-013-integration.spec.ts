import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopCommand, TaskId } from "../../src/bridge/types";
import ReviewView from "../../src/features/review/ReviewView.vue";

const taskId = "GAG-013-task" as TaskId;

function integrationBridge(commands: DesktopCommand[], executeFails = false, executeState = "ready_to_publish", activeState?: string) {
  return createFakeDesktopBridge({ onExecute(command) {
    commands.push(command);
    if (command.type === "review.status") return { success: "true", data: { snapshot: { head: "a".repeat(40), version: "v1", files: [] } } };
    if (command.type === "review.checkpoints") return { success: "true", data: { checkpoints: [{ id: "cp", taskId, attemptNumber: 1, commitSha: "b".repeat(40), treeSha: "c".repeat(40), headBefore: "a".repeat(40), selectionManifest: "[]", selectionHash: "hash", message: "checkpoint", createdAt: "2026-08-09T00:00:00Z" }] } };
    if (command.type === "integration.active") return { success: "true", data: { attempt: activeState ? { id: "attempt-1", taskId, repoRoot: "C:/repo", repoIdentity: "C:/repo/.git", sourceRef: "refs/heads/grok/task", sourceTipSha: "b".repeat(40), sourceRange: "[]", sourceDirty: false, sourceWorktreeDigest: "digest", targetRef: "refs/heads/main", expectedTargetSha: "a".repeat(40), commitMessage: "feat(GAG-013): squash task checkpoints", validationCommandsJson: "[]", validationDigest: "validation", approvalDigest: "approval", state: activeState, temporaryWorktreePath: "C:/managed/integration", resultCommitSha: "d".repeat(40), cleanupStatus: "not_started", createdAt: "now", updatedAt: "now" } : undefined } };
    if (command.type === "integration.preflight") return { success: "true", data: { plan: { attemptId: "attempt-1", taskId, sourceRef: "refs/heads/grok/task", sourceTipSha: "b".repeat(40), sourceRange: ["b".repeat(40)], sourceDirty: false, sourceWorktreeDigest: "digest", expectedFiles: ["shared.txt"], targetRef: "refs/heads/main", expectedTargetSha: "a".repeat(40), commitMessage: command.payload.commitMessage, validationCommands: [], validationDigest: "validation", approvalDigest: "approval" } } };
    if (command.type === "integration.execute") return executeFails ? { success: "false", error: { code: "INTEGRATION_STAGING_FAILED", message: "试合并失败", retryable: false, detailsRedacted: true, correlationId: "test" } } : { success: "true", data: { attempt: { id: "attempt-1", taskId, repoRoot: "C:/repo", repoIdentity: "C:/repo/.git", sourceRef: "refs/heads/grok/task", sourceTipSha: "b".repeat(40), sourceRange: "[]", sourceDirty: false, sourceWorktreeDigest: "digest", targetRef: "refs/heads/main", expectedTargetSha: "a".repeat(40), commitMessage: "feat(GAG-013): squash task checkpoints", validationCommandsJson: "[]", validationDigest: "validation", approvalDigest: "approval", state: executeState, temporaryWorktreePath: "C:/managed/integration", conflictSummaryJson: executeState === "conflicted" ? "[\"shared.txt\"]" : undefined, validationResultJson: "[{\"status\":\"passed\"}]", resultCommitSha: "d".repeat(40), cleanupStatus: "not_started", createdAt: "now", updatedAt: "now" } } };
    if (command.type === "integration.status") return { success: "true", data: { attempt: { id: "attempt-1", taskId, state: "cleanup_required", cleanupStatus: "not_started" } } };
    if (command.type === "integration.publish") return { success: "true", data: { attempt: { id: "attempt-1", taskId, state: "completed", resultCommitSha: "d".repeat(40), cleanupStatus: "not_started" } } };
    if (command.type === "integration.cleanup") return { success: "true", data: { attempt: { id: "attempt-1", taskId, state: "publish_rejected", cleanupStatus: "completed" } } };
    return { success: "true", data: {} };
  } });
}

describe("GAG-013 Review integration", () => {
  it("requires exact plan approval before isolated squash and separate publication", async () => {
    const commands: DesktopCommand[] = []; const wrapper = mount(ReviewView, { props: { bridge: integrationBridge(commands), taskId } }); await flushPromises();
    await wrapper.findAll("button").find((item) => item.text() === "集成预检")!.trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("refs/heads/main"); expect(wrapper.text()).toContain("无已配置命令"); expect(wrapper.text()).toContain("shared.txt"); expect(wrapper.text()).toContain("来源 Checkpoint 范围已冻结");
    const start = () => wrapper.findAll("button").find((item) => item.text().includes("开始隔离 Squash"))!;
    expect(start().attributes("disabled")).toBeDefined();
    await wrapper.find('.integration input[type="checkbox"]').setValue(true); await start().trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("ready_to_publish");
    await wrapper.findAll("button").find((item) => item.text() === "原子发布到目标引用")!.trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("已发布 dddddddddddd");
    expect(commands).toContainEqual({ type: "integration.execute", payload: { attemptId: "attempt-1", approvalDigest: "approval" } });
    expect(commands).toContainEqual({ type: "integration.publish", payload: { attemptId: "attempt-1", approvalDigest: "approval" } });
  });

  it("opens an isolated conflict worktree through the bridge", async () => {
    const commands: DesktopCommand[] = []; const wrapper = mount(ReviewView, { props: { bridge: integrationBridge(commands, false, "conflicted"), taskId } }); await flushPromises();
    await wrapper.findAll("button").find((item) => item.text() === "集成预检")!.trigger("click"); await flushPromises();
    await wrapper.find('.integration input[type="checkbox"]').setValue(true);
    await wrapper.findAll("button").find((item) => item.text().includes("开始隔离 Squash"))!.trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("冲突文件：shared.txt");
    await wrapper.findAll("button").find((item) => item.text().includes("打开临时 Worktree"))!.trigger("click");
    expect(commands).toContainEqual({ type: "integration.openWorktree", payload: { attemptId: "attempt-1" } });
  });

  it("discards stale approval and offers a fresh preflight after rejection", async () => {
    const commands: DesktopCommand[] = []; const wrapper = mount(ReviewView, { props: { bridge: integrationBridge(commands, false, "publish_rejected"), taskId } }); await flushPromises();
    await wrapper.findAll("button").find((item) => item.text() === "集成预检")!.trigger("click"); await flushPromises();
    await wrapper.find('.integration input[type="checkbox"]').setValue(true);
    await wrapper.findAll("button").find((item) => item.text().includes("开始隔离 Squash"))!.trigger("click"); await flushPromises();
    expect(wrapper.findAll("button").some((item) => item.text().includes("重新预检"))).toBe(false);
    await wrapper.findAll("button").find((item) => item.text().includes("清理临时资源"))!.trigger("click"); await flushPromises();
    await wrapper.findAll("button").find((item) => item.text().includes("重新预检"))!.trigger("click");
    expect(wrapper.findAll("button").some((item) => item.text() === "集成预检")).toBe(true);
  });

  it("recovers an unfinished attempt after the view is recreated", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(ReviewView, { props: { bridge: integrationBridge(commands, false, "ready_to_publish", "ready_to_publish"), taskId } });
    await flushPromises();
    expect(wrapper.text()).toContain("已恢复未完成的集成尝试");
    expect(wrapper.text()).toContain("ready_to_publish");
    await wrapper.findAll("button").find((item) => item.text() === "原子发布到目标引用")!.trigger("click");
    expect(commands).toContainEqual({ type: "integration.active", payload: { taskId } });
    expect(commands).toContainEqual({ type: "integration.publish", payload: { attemptId: "attempt-1", approvalDigest: "approval" } });
  });

  it("offers cleanup for a staging attempt restored before worktree creation", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(ReviewView, { props: { bridge: integrationBridge(commands, false, "ready_to_publish", "staging"), taskId } });
    await flushPromises();
    expect(wrapper.text()).toContain("staging");
    expect(wrapper.findAll("button").some((item) => item.text() === "清理临时资源")).toBe(true);
  });

  it("refreshes durable status and exposes cleanup after execute failure", async () => {
    const commands: DesktopCommand[] = []; const wrapper = mount(ReviewView, { props: { bridge: integrationBridge(commands, true), taskId } }); await flushPromises();
    await wrapper.findAll("button").find((item) => item.text() === "集成预检")!.trigger("click"); await flushPromises();
    await wrapper.find('.integration input[type="checkbox"]').setValue(true);
    await wrapper.findAll("button").find((item) => item.text().includes("开始隔离 Squash"))!.trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("试合并失败"); expect(wrapper.text()).toContain("cleanup_required"); expect(wrapper.text()).toContain("清理临时资源");
    expect(commands).toContainEqual({ type: "integration.status", payload: { attemptId: "attempt-1" } });
  });
});
