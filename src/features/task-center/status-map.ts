// GAG-007: Map TaskStatus enums to UI presentation — never parse labels.

import type { TaskStatus } from "../../bridge/types";
import type { TaskCapabilities, TaskGroupId } from "./types";

export type StatusIconKind =
  | "running"
  | "waiting"
  | "success"
  | "error"
  | "interrupted";

export interface TaskStatusPresentation {
  group: TaskGroupId;
  icon: StatusIconKind;
  label: string;
  capabilities: TaskCapabilities;
}

const PRESENTATION: Record<TaskStatus, TaskStatusPresentation> = {
  preparing: {
    group: "running",
    icon: "running",
    label: "准备中",
    capabilities: { canCancel: true, canRecover: false, canOpen: true },
  },
  running: {
    group: "running",
    icon: "running",
    label: "运行中",
    capabilities: { canCancel: true, canRecover: false, canOpen: true },
  },
  waiting_permission: {
    group: "needs_attention",
    icon: "waiting",
    label: "等待审批",
    capabilities: { canCancel: true, canRecover: false, canOpen: true },
  },
  integrating: {
    group: "running",
    icon: "running",
    label: "集成中",
    capabilities: { canCancel: false, canRecover: false, canOpen: true },
  },
  merged: {
    group: "completed",
    icon: "success",
    label: "已合并",
    capabilities: { canCancel: false, canRecover: false, canOpen: true },
  },
  archived: {
    group: "completed",
    icon: "success",
    label: "已归档",
    capabilities: { canCancel: false, canRecover: false, canOpen: true },
  },
  interrupted: {
    group: "failed_interrupted",
    icon: "interrupted",
    label: "已中断，可恢复",
    capabilities: { canCancel: false, canRecover: true, canOpen: true },
  },
};

export function presentTaskStatus(status: TaskStatus): TaskStatusPresentation {
  return PRESENTATION[status];
}

export function groupForStatus(status: TaskStatus): TaskGroupId {
  return PRESENTATION[status].group;
}

export function capabilitiesForStatus(status: TaskStatus): TaskCapabilities {
  return PRESENTATION[status].capabilities;
}

/** Sort priority: needs-attention first, then running, then others. */
export function groupSortRank(group: TaskGroupId): number {
  switch (group) {
    case "needs_attention":
      return 0;
    case "running":
      return 1;
    case "completed":
      return 2;
    case "failed_interrupted":
      return 3;
  }
}

export function isKnownTaskStatus(value: string): value is TaskStatus {
  return value in PRESENTATION;
}
