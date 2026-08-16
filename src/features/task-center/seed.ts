// GAG-007: Seed data for DEV fixture / tests. Not used on production Tauri path.

import type {
  BootstrapSnapshot,
  Project,
  ProjectId,
  Task,
  TaskId,
} from "../../bridge/types";

function project(id: string, path: string): Project {
  return {
    id: id as ProjectId,
    path,
    displayPath: path,
    lastOpenedAt: "2026-04-01T10:00:00.000Z",
  };
}

function task(
  partial: Omit<Task, "workspaceKind" | "createdAt" | "updatedAt"> &
    Partial<Pick<Task, "workspaceKind" | "createdAt" | "updatedAt">>,
): Task {
  return {
    workspaceKind: "worktree",
    createdAt: "2026-04-01T09:00:00.000Z",
    updatedAt: "2026-04-01T12:00:00.000Z",
    ...partial,
  };
}

export function createTaskCenterSeedSnapshot(): BootstrapSnapshot {
  const projects = [
    project("proj-alpha", "D:/work/alpha"),
    project("proj-beta", "D:/work/beta"),
  ];

  const completedTasks: Task[] = [
    task({
      id: "task-merged-1" as TaskId,
      projectId: "proj-beta" as ProjectId,
      title: "已完成的文档整理",
      status: "merged",
      updatedAt: "2026-04-01T11:00:00.000Z",
    }),
  ];

  const activeTasks: Task[] = [
    task({
      id: "task-wait-1" as TaskId,
      projectId: "proj-alpha" as ProjectId,
      title: "等待审批：写入配置",
      status: "waiting_permission",
      updatedAt: "2026-04-01T12:30:00.000Z",
    }),
    task({
      id: "task-run-1" as TaskId,
      projectId: "proj-alpha" as ProjectId,
      title: "实现 Task Center UI",
      status: "running",
      mode: "agent",
      updatedAt: "2026-04-01T12:25:00.000Z",
    }),
    task({
      id: "task-prep-1" as TaskId,
      projectId: "proj-beta" as ProjectId,
      title: "准备集成环境",
      status: "preparing",
      updatedAt: "2026-04-01T12:20:00.000Z",
    }),
    task({
      id: "task-int-1" as TaskId,
      projectId: "proj-alpha" as ProjectId,
      title: "中断的重构任务",
      status: "interrupted",
      interruptReason: "应用退出时会话仍在运行",
      updatedAt: "2026-04-01T10:00:00.000Z",
    }),
  ];

  return {
    productName: "Grok ACP GUI (task-center fixture)",
    version: "0.0.0-test",
    platform: "win32",
    ready: true,
    runtime: { status: "ready", version: "fixture", authenticated: true },
    capabilities: { models: [], modes: [], slashCommands: [] },
    projects,
    activeTasks,
    completedTasks,
    bindings: [
      {
        taskId: "task-run-1" as TaskId,
        sessionId: "sess-run-1" as import("../../bridge/types").SessionId,
        lastSeq: 3,
        state: "active",
      },
    ],
    worktrees: [
      {
        id: "wt-1",
        taskId: "task-run-1" as TaskId,
        repoRoot: "D:/work/alpha",
        path: "D:/work/alpha/.grok/worktrees/task-run-1",
        displayPath: ".grok/worktrees/task-run-1",
        branch: "feat/gag-007",
        baseBranch: "main",
        baseCommit: "abc1234",
        ownership: "managed",
        state: "dirty",
      },
    ],
    recoveryItems: [],
    settings: [],
    recoveryPerformed: false,
    tasksInterrupted: 1,
  };
}
