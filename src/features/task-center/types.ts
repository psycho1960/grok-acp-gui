// GAG-007: Feature-local view models for Task Center.
// Maps onto DesktopBridge Task / bootstrap entities without inventing domain state.

import type {
  ProjectId,
  SessionId,
  TaskId,
  TaskStatus,
  WorkspaceKind,
  WorktreeState,
} from "../../bridge/types";

/** UI grouping used by Task Center (UI-TASK-001 / GAG-007 §6). */
export type TaskGroupId =
  | "needs_attention"
  | "running"
  | "completed"
  | "failed_interrupted";

export type TaskCenterLoadState =
  | "idle"
  | "loading"
  | "ready"
  | "error"
  | "stale";

/** UI-only capability flags derived from TaskStatus — not backend authorization. */
export interface TaskCapabilities {
  canCancel: boolean;
  canRecover: boolean;
  canOpen: boolean;
}

export type UpdatedWithin = "any" | "1h" | "24h" | "7d";

export interface TaskCenterFilters {
  query: string;
  status: TaskStatus | "all";
  projectId: ProjectId | "all";
  updatedWithin: UpdatedWithin;
  /** Optional group focus from left nav. */
  group: TaskGroupId | "all";
}

export interface TaskViewModel {
  id: TaskId;
  projectId: ProjectId;
  projectLabel: string;
  title: string;
  status: TaskStatus;
  workspaceKind: WorkspaceKind;
  mode?: string;
  model?: string;
  createdAt: string;
  updatedAt: string;
  interruptReason?: string;
  /** Optional phase/stage label when available from activity events. */
  phase?: string;
  /** Latest activity summary text (untrusted). */
  latestActivity?: string;
  queuePosition?: number;
  hasLiveSession?: boolean;
  sessionId?: SessionId;
  sessionState?: string;
  worktreeDisplayPath?: string;
  branch?: string;
  baseBranch?: string;
  worktreeState?: WorktreeState;
  /** Per-task isolation error; does not fail the whole list. */
  localError?: string;
  /** Last applied event seq for this task (ordering guard). */
  lastSeq: number;
}

export interface TaskDetailViewModel {
  task: TaskViewModel;
  /** From task.open result when available. */
  openTitle?: string;
  openStatus?: string;
  compatibilityError?: string;
}

export interface TaskCenterSnapshotMeta {
  /** Monotonic counter for bootstrap/list refreshes. */
  version: number;
  /** Highest session event seq observed. */
  maxSeq: number;
  /** ISO timestamp of last successful refresh. */
  refreshedAt: string | null;
}

export const DEFAULT_FILTERS: TaskCenterFilters = {
  query: "",
  status: "all",
  projectId: "all",
  updatedWithin: "any",
  group: "all",
};

export const TASK_GROUP_ORDER: readonly TaskGroupId[] = [
  "needs_attention",
  "running",
  "completed",
  "failed_interrupted",
] as const;

export const TASK_GROUP_LABELS: Record<TaskGroupId, string> = {
  needs_attention: "等待处理",
  running: "运行中",
  completed: "已完成",
  failed_interrupted: "失败/中断",
};
