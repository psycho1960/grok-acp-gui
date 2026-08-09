// GAG-010B / goal: the single Renderer mapping for Create Task and Conversation.
// Pure logic only; the backend independently validates and resolves the real cwd:
// ask → direct (可写当前目录), agent/plan → worktree (隔离 Worktree).

import type { WorkspaceStrategy } from "../../bridge/types";

export type { WorkspaceStrategy } from "../../bridge/types";

export const WORKTREE_NOT_READY_MESSAGE =
  "隔离 Worktree 尚未创建，本任务不会回落到原工作区。";

export const MODE_WORKSPACE_DEFAULTS = Object.freeze({
  ask: "direct",
  agent: "worktree",
  plan: "worktree",
} satisfies Record<string, WorkspaceStrategy>);

/** Chinese labels for the workspace strategy control. */
export const WORKSPACE_STRATEGY_OPTIONS: Array<{
  value: WorkspaceStrategy;
  label: string;
}> = [
  { value: "worktree", label: "隔离 Worktree" },
  { value: "readonly", label: "只读当前目录" },
  { value: "direct", label: "当前目录可写" },
];

/**
 * Map a session mode to the workspace strategy that follows it.
 * ask → direct; agent/plan → worktree. Unknown modes stay unchanged (null),
 * and readonly is never auto-selected — it is user-chosen only.
 */
export function workspaceStrategyForMode(
  mode: string | null | undefined,
): WorkspaceStrategy | null {
  if (!mode) return null;
  return MODE_WORKSPACE_DEFAULTS[mode as keyof typeof MODE_WORKSPACE_DEFAULTS] ?? null;
}

/** Whether a strategy string is a valid persisted value. */
export function isWorkspaceStrategy(value: unknown): value is WorkspaceStrategy {
  return (
    value === "worktree" || value === "readonly" || value === "direct"
  );
}
