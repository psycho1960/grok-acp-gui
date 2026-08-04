// GAG-007: Pure group / sort / filter helpers for Task Center.

import type { TaskStatus } from "../../bridge/types";
import { groupForStatus, groupSortRank } from "./status-map";
import type {
  TaskCenterFilters,
  TaskGroupId,
  TaskViewModel,
  UpdatedWithin,
} from "./types";
import { TASK_GROUP_ORDER } from "./types";

export interface TaskGroupBucket {
  id: TaskGroupId;
  tasks: TaskViewModel[];
}

function parseTime(iso: string): number {
  const t = Date.parse(iso);
  return Number.isFinite(t) ? t : 0;
}

function matchesUpdatedWithin(
  updatedAt: string,
  within: UpdatedWithin,
  nowMs: number,
): boolean {
  if (within === "any") return true;
  const updated = parseTime(updatedAt);
  if (!updated) return true;
  const windows: Record<Exclude<UpdatedWithin, "any">, number> = {
    "1h": 60 * 60 * 1000,
    "24h": 24 * 60 * 60 * 1000,
    "7d": 7 * 24 * 60 * 60 * 1000,
  };
  return nowMs - updated <= windows[within];
}

export function matchesFilters(
  task: TaskViewModel,
  filters: TaskCenterFilters,
  nowMs: number = Date.now(),
): boolean {
  if (filters.status !== "all" && task.status !== filters.status) {
    return false;
  }
  if (filters.projectId !== "all" && task.projectId !== filters.projectId) {
    return false;
  }
  if (filters.group !== "all" && groupForStatus(task.status) !== filters.group) {
    return false;
  }
  if (!matchesUpdatedWithin(task.updatedAt, filters.updatedWithin, nowMs)) {
    return false;
  }
  const q = filters.query.trim().toLowerCase();
  if (q) {
    const haystack = [
      task.title,
      task.projectLabel,
      task.id,
      task.phase ?? "",
      task.latestActivity ?? "",
      task.branch ?? "",
    ]
      .join(" ")
      .toLowerCase();
    if (!haystack.includes(q)) return false;
  }
  return true;
}

/**
 * Sort: needs-attention → running → others by group rank,
 * then updatedAt desc, then Task ID ascending for stability.
 */
export function compareTasks(a: TaskViewModel, b: TaskViewModel): number {
  const groupDiff =
    groupSortRank(groupForStatus(a.status)) -
    groupSortRank(groupForStatus(b.status));
  if (groupDiff !== 0) return groupDiff;
  const timeDiff = parseTime(b.updatedAt) - parseTime(a.updatedAt);
  if (timeDiff !== 0) return timeDiff;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

export function filterAndSortTasks(
  tasks: readonly TaskViewModel[],
  filters: TaskCenterFilters,
  nowMs: number = Date.now(),
): TaskViewModel[] {
  return tasks.filter((t) => matchesFilters(t, filters, nowMs)).sort(compareTasks);
}

export function groupTasks(tasks: readonly TaskViewModel[]): TaskGroupBucket[] {
  const buckets = new Map<TaskGroupId, TaskViewModel[]>();
  for (const id of TASK_GROUP_ORDER) buckets.set(id, []);
  for (const task of tasks) {
    const group = groupForStatus(task.status);
    buckets.get(group)!.push(task);
  }
  for (const list of buckets.values()) list.sort(compareTasks);
  return TASK_GROUP_ORDER.map((id) => ({ id, tasks: buckets.get(id)! }));
}

export function countByGroup(
  tasks: readonly TaskViewModel[],
): Record<TaskGroupId, number> {
  const counts: Record<TaskGroupId, number> = {
    needs_attention: 0,
    running: 0,
    completed: 0,
    failed_interrupted: 0,
  };
  for (const task of tasks) {
    counts[groupForStatus(task.status)] += 1;
  }
  return counts;
}

export function countByStatus(
  tasks: readonly TaskViewModel[],
): Partial<Record<TaskStatus, number>> {
  const counts: Partial<Record<TaskStatus, number>> = {};
  for (const task of tasks) {
    counts[task.status] = (counts[task.status] ?? 0) + 1;
  }
  return counts;
}
