import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import RecoveryCenter from "../../src/features/worktrees/RecoveryCenter.vue";
import type {
  DesktopBridge,
  DesktopCommand,
  RecoveryActionPlan,
  RecoveryIssue,
  TaskId,
} from "../../src/bridge/types";

const interrupted: RecoveryIssue = {
  issueId: "issue-task", revision: 1, scanId: "scan-1", stableKey: "interrupted-task:task-1",
  kind: "interrupted_task", severity: "immediate", status: "detected", taskId: "task-1" as TaskId,
  resourceId: "task-1", evidence: { previousStatus: "running", hasSession: true },
  impact: "The task has no proven live ACP process.", recommendedAction: "Resume the persisted session.",
  safeActions: ["resume_session", "retain"], detectedAt: "2026-08-10T00:00:00Z",
};
const worktree: RecoveryIssue = {
  issueId: "issue-worktree", revision: 3, scanId: "scan-1", stableKey: "worktree:wt-1",
  kind: "worktree_mismatch", severity: "deferred", status: "detected", taskId: "task-1" as TaskId,
  resourceId: "wt-1", canonicalPath: "C:/managed/repo/task-1",
  evidence: { ownership: "managed", state: "closing", repoIdentity: "repo-1" },
  impact: "Database, Git and filesystem registration are not aligned.", recommendedAction: "Use verified cleanup.",
  safeActions: ["verify_and_cleanup", "show_location", "retain"], detectedAt: "2026-08-10T00:01:00Z",
};

function actionPlan(issue: RecoveryIssue, action: RecoveryActionPlan["actionKind"]): RecoveryActionPlan {
  return {
    id: `plan-${issue.issueId}-${action}`, issueId: issue.issueId, issueRevision: issue.revision + 1,
    actionKind: action, resourceIdentity: issue.resourceId, canonicalPath: issue.canonicalPath,
    expectedState: { recoveryBundleId: "bundle-record-1", canonicalPath: issue.canonicalPath },
    steps: ["revalidate issue revision", "verify independent recovery bundle", "remove exact managed Worktree"],
    destructiveLevel: action === "verify_and_cleanup" ? "destructive" : "non_destructive",
    approvalDigest: `approval-${issue.issueId}`, expiresAtEpoch: 1_900_000_000, createdAt: "now",
  };
}

function bridge(commands: DesktopCommand[], issues = [interrupted, worktree]): DesktopBridge {
  return {
    bootstrap: async () => ({ productName: "Grok", version: "1", platform: "win32", ready: true, dbError: null, projects: [], tasks: [], sessionBindings: [], worktrees: [], recoveryItems: [], settings: [], runtime: { status: "ready", authenticated: true }, capabilities: { protocolVersion: "1", image: false, plan: false, permissions: false, sessionResume: true } }),
    subscribe: async () => () => undefined,
    execute: async (command) => {
      commands.push(command);
      if (command.type === "recovery.history") {
        return {
          success: "true",
          data: {
            history: {
              scans: [{ id: "scan-1", triggerKind: "startup", startedAt: "now", completedAt: "now", issueCount: issues.length }],
              issues,
              plans: [],
              bundles: [],
              steps: [],
            },
          },
        };
      }
      if (command.type === "recovery.scan") return { success: "true", data: { issues } };
      if (command.type === "recovery.prepareAction") {
        const issue = issues.find((item) => item.issueId === command.payload.issueId)!;
        return { success: "true", data: { plan: actionPlan(issue, command.payload.action) } };
      }
      if (command.type === "recovery.executeAction") {
        const issue = issues.find((item) => command.payload.planId.includes(item.issueId))!;
        return { success: "true", data: { issue: { ...issue, revision: issue.revision + 2, status: command.payload.planId.endsWith("retain") ? "retained" : "resolved" } } };
      }
      return { success: "true", data: {} };
    },
  };
}

describe("GAG-014 Recovery Center", () => {
  it("groups issues and presents evidence, impact and only backend-provided actions", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(RecoveryCenter, { props: { bridge: bridge(commands) } });
    await flushPromises();
    expect(wrapper.text()).toContain("需立即处理");
    expect(wrapper.text()).toContain("可安全延后");
    await wrapper.findAll(".issue-row").find((item) => item.text().includes("task-1"))!.trigger("click");
    expect(wrapper.text()).toContain("previousStatus");
    expect(wrapper.text()).toContain("恢复会话");
    expect(wrapper.text()).not.toContain("重新登记");
    expect(commands[0]).toEqual({ type: "recovery.history", payload: {} });
  });

  it("requires explicit approval for an exact destructive plan with verified bundle state", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(RecoveryCenter, { props: { bridge: bridge(commands) } });
    await flushPromises();
    await wrapper.findAll(".issue-row").find((item) => item.text().includes("wt-1"))!.trigger("click");
    await wrapper.findAll("button").find((item) => item.text() === "验证后清理")!.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("C:/managed/repo/task-1");
    expect(wrapper.text()).toContain("bundle-record-1");
    const execute = wrapper.get('[data-testid="recovery-execute"]');
    expect(execute.attributes("disabled")).toBeDefined();
    await wrapper.get('.approval input[type="checkbox"]').setValue(true);
    await execute.trigger("click");
    await flushPromises();
    expect(commands).toContainEqual({ type: "recovery.prepareAction", payload: { issueId: "issue-worktree", revision: 3, action: "verify_and_cleanup" } });
    expect(commands).toContainEqual({ type: "recovery.executeAction", payload: { planId: "plan-issue-worktree-verify_and_cleanup", approvalDigest: "approval-issue-worktree" } });
    expect(wrapper.text()).toContain("状态 resolved");
  });

  it("rescans without cleanup and reports a low-risk batch result", async () => {
    const commands: DesktopCommand[] = [];
    const wrapper = mount(RecoveryCenter, { props: { bridge: bridge(commands) } });
    await flushPromises();
    await wrapper.get('[data-testid="recovery-scan"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("未执行任何清理");
    await wrapper.findAll("button").find((item) => item.text() === "保留全部低风险项")!.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("批量保留完成：1 项");
    expect(commands).toContainEqual({ type: "recovery.prepareAction", payload: { issueId: "issue-worktree", revision: 3, action: "retain" } });
  });
});
