// GAG-010 / goal: mode ↔ workspace strategy linkage — pure logic, no Tauri
// dependencies. Mirrors the create-task dialog's established mapping:
// ask → direct (可写当前目录), agent/plan → worktree (隔离 Worktree).

export type WorkspaceStrategy = "worktree" | "readonly" | "direct";

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
  switch (mode) {
    case "ask":
      return "direct";
    case "agent":
    case "plan":
      return "worktree";
    default:
      return null;
  }
}

/** Whether a strategy string is a valid persisted value. */
export function isWorkspaceStrategy(value: unknown): value is WorkspaceStrategy {
  return (
    value === "worktree" || value === "readonly" || value === "direct"
  );
}
