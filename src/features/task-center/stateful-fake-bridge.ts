// In-memory DesktopBridge for first-use closed-loop tests and empty-shell demos.

import { createFakeDesktopBridge, fakeError } from "../../bridge/fake-bridge";
import type {
  BootstrapSnapshot,
  DesktopBridge,
  DesktopCommand,
  DesktopResult,
  Project,
  ProjectId,
  Task,
  TaskId,
} from "../../bridge/types";

export interface StatefulFakeOptions {
  /** Initial bootstrap seed (projects/tasks). Default: empty ready shell. */
  initial?: Partial<BootstrapSnapshot>;
  /**
   * Path keywords for simulate rules:
   * - includes "missing" → invalid
   * - includes "not-a-dir" → invalid
   * - includes "nongit" → non-git success
   * - includes "fail-create" as project path → create task fails
   */
}

export function createStatefulTaskCenterBridge(
  options: StatefulFakeOptions = {},
): DesktopBridge & {
  pushEvent: (event: import("../../bridge/types").TypedDesktopEvent) => void;
  getState: () => { projects: Project[]; tasks: Task[] };
} {
  const projects: Project[] = [...(options.initial?.projects ?? [])];
  const tasks: Task[] = [...(options.initial?.activeTasks ?? [])];
  let seq = 1;

  function snapshot(): BootstrapSnapshot {
    const base = options.initial ?? {};
    return {
      productName: base.productName ?? "Grok ACP GUI (stateful fake)",
      version: base.version ?? "0.0.0-test",
      platform: base.platform ?? "win32",
      ready: base.ready ?? true,
      runtime: base.runtime ?? { status: "ready", authenticated: true },
      capabilities: base.capabilities ?? {
        models: [{ modelId: "grok", name: "Grok" }],
        modes: [
          { id: "agent", name: "Agent" },
          { id: "plan", name: "Plan" },
          { id: "ask", name: "Ask" },
        ],
        slashCommands: [],
      },
      projects: [...projects],
      activeTasks: [...tasks],
      bindings: base.bindings ?? [],
      worktrees: base.worktrees ?? [],
      recoveryItems: base.recoveryItems ?? [],
      settings: base.settings ?? [],
      recoveryPerformed: base.recoveryPerformed ?? false,
      tasksInterrupted: base.tasksInterrupted ?? 0,
      dbError: base.dbError,
    };
  }

  function onExecute(command: DesktopCommand): DesktopResult {
    if (command.type === "workspace.inspect") {
      const path = command.payload.path.trim();
      if (!path || /missing|not-exist|ENOENT/i.test(path)) {
        return {
          success: "false",
          error: fakeError({
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Directory does not exist or is not accessible",
          }),
        };
      }
      if (/not-a-dir|file\.txt/i.test(path)) {
        return {
          success: "false",
          error: fakeError({
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Path is not a directory",
          }),
        };
      }
      const isGit = !/nongit/i.test(path);
      return {
        success: "true",
        data: {
          repoRoot: isGit ? path : path,
          branch: isGit ? "main" : "unknown",
          dirty: false,
          isGit,
        },
      };
    }

    if (command.type === "project.open") {
      const path = command.payload.path.trim();
      if (!path || /missing|not-exist/i.test(path)) {
        return {
          success: "false",
          error: fakeError({
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Directory does not exist or is not accessible",
          }),
        };
      }
      if (/not-a-dir/i.test(path)) {
        return {
          success: "false",
          error: fakeError({
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Path is not a directory",
          }),
        };
      }
      const nonGit = /nongit/i.test(path);
      const existing = projects.find((p) => p.path === path);
      if (existing) {
        existing.lastOpenedAt = new Date().toISOString();
        return {
          success: "true",
          data: {
            projectId: existing.id,
            path: existing.path,
            displayPath: existing.displayPath,
            repoRoot: nonGit ? undefined : existing.repoRoot ?? path,
            nonGit,
          },
        };
      }
      const id = `proj-fake-${seq++}` as ProjectId;
      const project: Project = {
        id,
        path,
        displayPath: path.split(/[/\\]/).filter(Boolean).slice(-2).join("/") || path,
        repoRoot: nonGit ? undefined : path,
        trustedAt: new Date().toISOString(),
        lastOpenedAt: new Date().toISOString(),
      };
      projects.unshift(project);
      return {
        success: "true",
        data: {
          projectId: id,
          path,
          displayPath: project.displayPath,
          repoRoot: project.repoRoot,
          nonGit,
        },
      };
    }

    if (command.type === "task.create") {
      const project = projects.find((p) => p.id === command.payload.projectId);
      if (!project) {
        return {
          success: "false",
          error: fakeError({
            code: "PROJECT_NOT_FOUND",
            message: "Project not found",
          }),
        };
      }
      if (/fail-create/i.test(project.path)) {
        return {
          success: "false",
          error: fakeError({
            code: "DB_QUERY_FAILED",
            message: "创建任务失败：数据库错误",
            retryable: true,
          }),
        };
      }
      if (!command.payload.prompt?.trim()) {
        return {
          success: "false",
          error: fakeError({
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Task prompt is required",
          }),
        };
      }
      const id = `task-fake-${seq++}` as TaskId;
      const now = new Date().toISOString();
      const task: Task = {
        id,
        projectId: project.id,
        title: command.payload.title,
        status: "preparing",
        workspaceKind:
          command.payload.workspaceStrategy === "direct"
            ? "direct"
            : command.payload.workspaceStrategy === "readonly"
              ? "readonly"
              : "worktree",
        mode: command.payload.mode,
        model: command.payload.model,
        reasoning: command.payload.reasoning,
        createdAt: now,
        updatedAt: now,
      };
      tasks.unshift(task);
      return {
        success: "true",
        data: {
          taskId: id,
          task: {
            id,
            projectId: project.id,
            title: task.title,
            status: task.status,
            createdAt: now,
          },
        },
      };
    }

    if (command.type === "task.open") {
      const task = tasks.find((t) => t.id === command.payload.taskId);
      if (!task) {
        return {
          success: "false",
          error: fakeError({ message: "任务不存在" }),
        };
      }
      return {
        success: "true",
        data: {
          taskId: task.id,
          title: task.title,
          status: task.status,
        },
      };
    }

    if (command.type === "turn.cancel") {
      return { success: "true", data: { acknowledged: "turn.cancel" } };
    }

    return { success: "true", data: { acknowledged: command.type } };
  }

  const bridge = createFakeDesktopBridge({
    bootstrapSnapshot: snapshot(),
    onExecute,
  });

  // bootstrap must return live arrays — wrap
  return {
    async bootstrap() {
      return snapshot();
    },
    execute: bridge.execute.bind(bridge),
    subscribe: bridge.subscribe.bind(bridge),
    pushEvent: bridge.pushEvent.bind(bridge),
    getState: () => ({ projects: [...projects], tasks: [...tasks] }),
  };
}
