// GAG-007: Flatten grouped tasks into virtualized rows (headers + cards).

import type { TaskGroupBucket } from "./grouping";
import type { TaskGroupId, TaskViewModel } from "./types";
import { TASK_GROUP_LABELS } from "./types";

export type TaskListRow =
  | {
      kind: "header";
      key: string;
      groupId: TaskGroupId;
      label: string;
      count: number;
    }
  | {
      kind: "task";
      key: string;
      task: TaskViewModel;
    };

export function buildGroupedListRows(
  groups: readonly TaskGroupBucket[],
): TaskListRow[] {
  const rows: TaskListRow[] = [];
  for (const group of groups) {
    if (group.tasks.length === 0) continue;
    rows.push({
      kind: "header",
      key: `header:${group.id}`,
      groupId: group.id,
      label: TASK_GROUP_LABELS[group.id],
      count: group.tasks.length,
    });
    for (const task of group.tasks) {
      rows.push({
        kind: "task",
        key: `task:${task.id}`,
        task,
      });
    }
  }
  return rows;
}
