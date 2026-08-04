// GAG-007: Hash-based deep links for Task Center (no vue-router).

const TASK_CENTER_PREFIX = "#task-center";

export interface TaskCenterRoute {
  active: boolean;
  taskId: string | null;
}

export function parseTaskCenterHash(hash: string): TaskCenterRoute {
  const raw = hash.startsWith("#") ? hash : `#${hash}`;
  if (raw === TASK_CENTER_PREFIX || raw === `${TASK_CENTER_PREFIX}/`) {
    return { active: true, taskId: null };
  }
  if (raw.startsWith(`${TASK_CENTER_PREFIX}/`)) {
    const rest = raw.slice(`${TASK_CENTER_PREFIX}/`.length);
    const taskId = decodeURIComponent(rest.split(/[/?#]/)[0] ?? "").trim();
    return { active: true, taskId: taskId || null };
  }
  return { active: false, taskId: null };
}

export function buildTaskCenterHash(taskId?: string | null): string {
  if (taskId) return `${TASK_CENTER_PREFIX}/${encodeURIComponent(taskId)}`;
  return TASK_CENTER_PREFIX;
}

export function applyTaskCenterHash(taskId?: string | null): void {
  if (typeof window === "undefined") return;
  const next = buildTaskCenterHash(taskId);
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}
