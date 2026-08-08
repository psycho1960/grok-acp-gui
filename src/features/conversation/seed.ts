// GAG-008: Seed data for ConversationFixture / offline UI development.

import type {
  BootstrapSnapshot,
  ProjectId,
  TaskId,
} from "../../bridge/types";
import {
  FIX_SESSION,
  FIX_TASK,
  fixtureConversationEvents,
  fixtureSessionSnapshot,
} from "./fixtures";
import type { SessionTimelineSnapshot } from "./types";

const PROJ = "proj-conv" as ProjectId;

export function createConversationSeedSnapshot(): BootstrapSnapshot {
  return {
    productName: "Grok ACP GUI (conversation fixture)",
    version: "0.0.0-test",
    platform: "win32",
    ready: true,
    runtime: { status: "ready", authenticated: true, version: "fake" },
    capabilities: {
      models: [
        { modelId: "grok-4.5", name: "grok-4.5", reasoningEffort: "high" },
        { modelId: "deepseek", name: "deepseek (deepseek-v4-pro)", reasoningEffort: "max" },
        { modelId: "luna", name: "gpt-5.6-luna", reasoningEffort: "medium" },
      ],
      modes: [
        { id: "agent", name: "智能体" },
        { id: "plan", name: "计划" },
        { id: "ask", name: "问答" },
      ],
      slashCommands: [
        { name: "init", description: "初始化一个新项目", acceptsInput: false },
        { name: "plan", description: "为变更制定计划", acceptsInput: true },
        { name: "share", description: "分享当前会话", acceptsInput: false },
      ],
    },
    projects: [
      {
        id: PROJ,
        path: "D:/demo",
        displayPath: "demo",
        lastOpenedAt: "2026-04-01T10:00:00.000Z",
      },
    ],
    activeTasks: [
      {
        id: FIX_TASK,
        projectId: PROJ,
        title: "对话演示",
        status: "running",
        workspaceKind: "worktree",
        mode: "agent",
        model: "grok-4.5",
        reasoning: "high",
        createdAt: "2026-04-01T11:00:00.000Z",
        updatedAt: "2026-04-01T12:00:00.000Z",
      },
    ],
    bindings: [
      {
        taskId: FIX_TASK,
        sessionId: FIX_SESSION,
        lastSeq: 26,
        state: "active",
      },
    ],
    worktrees: [],
    recoveryItems: [],
    settings: [],
    recoveryPerformed: false,
    tasksInterrupted: 0,
  };
}

export function createSeedTimeline(
  taskId: TaskId = FIX_TASK,
): SessionTimelineSnapshot {
  return fixtureSessionSnapshot({
    taskId,
    sessionId: FIX_SESSION,
    title: "对话演示",
    status: "running",
    cursor: 5,
    attempt: 1,
    mode: "agent",
    model: "grok-4.5",
    reasoning: "high",
  });
}

export function createLiveSeedEvents() {
  return fixtureConversationEvents();
}
