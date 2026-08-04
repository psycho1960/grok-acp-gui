// GAG-007: Hash-based deep links for Task Center (no vue-router).

import type { TaskGroupId } from "./types";
import { TASK_GROUP_ORDER } from "./types";

const TASK_CENTER_PREFIX = "#task-center";

export interface TaskCenterRoute {
  active: boolean;
  taskId: string | null;
  group: TaskGroupId | "all" | null;
}

function isTaskGroupId(value: string): value is TaskGroupId {
  return (TASK_GROUP_ORDER as readonly string[]).includes(value);
}

function parseQuery(query: string): { group: TaskGroupId | "all" | null } {
  if (!query) return { group: null };
  const params = new URLSearchParams(query.startsWith("?") ? query.slice(1) : query);
  const raw = params.get("group");
  if (!raw || raw === "all") return { group: raw === "all" ? "all" : null };
  if (isTaskGroupId(raw)) return { group: raw };
  return { group: null };
}

/**
 * Accepts:
 * - #task-center
 * - #task-center?group=running
 * - #task-center/<taskId>
 * - #task-center/<taskId>?group=running
 */
export function parseTaskCenterHash(hash: string): TaskCenterRoute {
  const raw = hash.startsWith("#") ? hash : `#${hash}`;
  if (!raw.startsWith(TASK_CENTER_PREFIX)) {
    return { active: false, taskId: null, group: null };
  }

  const after = raw.slice(TASK_CENTER_PREFIX.length);
  if (after === "" || after === "/") {
    return { active: true, taskId: null, group: null };
  }

  if (after.startsWith("?")) {
    const { group } = parseQuery(after);
    return { active: true, taskId: null, group };
  }

  if (after.startsWith("/")) {
    const rest = after.slice(1);
    const qIndex = rest.indexOf("?");
    const idPart = qIndex >= 0 ? rest.slice(0, qIndex) : rest;
    const query = qIndex >= 0 ? rest.slice(qIndex) : "";
    const taskId = decodeURIComponent(idPart.split(/[/#]/)[0] ?? "").trim();
    const { group } = parseQuery(query);
    return { active: true, taskId: taskId || null, group };
  }

  return { active: true, taskId: null, group: null };
}

export function buildTaskCenterHash(
  taskId?: string | null,
  group?: TaskGroupId | "all" | null,
): string {
  let base = TASK_CENTER_PREFIX;
  if (taskId) base += `/${encodeURIComponent(taskId)}`;
  if (group && group !== "all") {
    base += `?group=${encodeURIComponent(group)}`;
  } else if (group === "all") {
    // Explicit all only when needed to clear a previous group without task id.
    // Prefer bare #task-center for all.
  }
  return base;
}

export function applyTaskCenterHash(
  taskId?: string | null,
  group?: TaskGroupId | "all" | null,
): void {
  if (typeof window === "undefined") return;
  const next = buildTaskCenterHash(taskId, group);
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}
