// GAG-007: Pinia store for Task Center — snapshot merge, stale, filters.

import { computed, ref, shallowRef } from "vue";
import { defineStore } from "pinia";
import type {
  DesktopBridge,
  Project,
  TaskId,
  TaskOpenResult,
  TaskStatus,
} from "../../bridge/types";
import {
  createTaskCenterFacade,
  parseSnapshotTasks,
  type TaskCenterBridgeEvent,
  type TaskCenterFacade,
} from "./task-bridge-facade";
import {
  countByGroup,
  filterAndSortTasks,
  groupTasks,
} from "./grouping";
import { isKnownTaskStatus } from "./status-map";
import {
  DEFAULT_FILTERS,
  type TaskCenterFilters,
  type TaskCenterLoadState,
  type TaskDetailViewModel,
  type TaskViewModel,
} from "./types";

const LIVE_ANNOUNCE_THROTTLE_MS = 1500;

export const useTaskCenterStore = defineStore("task-center", () => {
  const loadState = ref<TaskCenterLoadState>("idle");
  const errorMessage = ref<string | null>(null);
  const tasksById = shallowRef<Map<TaskId, TaskViewModel>>(new Map());
  const projects = shallowRef<Project[]>([]);
  const version = ref(0);
  const maxSeq = ref(0);
  const refreshedAt = ref<string | null>(null);
  const filters = ref<TaskCenterFilters>({ ...DEFAULT_FILTERS });
  const selectedTaskId = ref<TaskId | null>(null);
  const detail = ref<TaskDetailViewModel | null>(null);
  const detailLoading = ref(false);
  const cancelPendingId = ref<TaskId | null>(null);
  const liveMessage = ref("");
  let lastAnnounceAt = 0;
  let facade: TaskCenterFacade | null = null;
  let unsubscribe: (() => void) | null = null;
  let disposed = false;

  const allTasks = computed(() => Array.from(tasksById.value.values()));

  const visibleTasks = computed(() =>
    filterAndSortTasks(allTasks.value, filters.value),
  );

  const groups = computed(() => groupTasks(visibleTasks.value));

  const counts = computed(() => countByGroup(allTasks.value));

  const selectedTask = computed(() => {
    if (!selectedTaskId.value) return null;
    return tasksById.value.get(selectedTaskId.value) ?? null;
  });

  const projectOptions = computed(() =>
    projects.value.map((p) => ({
      value: p.id as string,
      label: p.displayPath || p.path || p.id,
    })),
  );

  function replaceTasks(next: TaskViewModel[]): void {
    const map = new Map<TaskId, TaskViewModel>();
    for (const task of next) map.set(task.id, task);
    tasksById.value = map;
  }

  function upsertTask(task: TaskViewModel): void {
    const map = new Map(tasksById.value);
    map.set(task.id, task);
    tasksById.value = map;
  }

  function announce(message: string, force = false): void {
    const now = Date.now();
    if (!force && now - lastAnnounceAt < LIVE_ANNOUNCE_THROTTLE_MS) return;
    lastAnnounceAt = now;
    liveMessage.value = message;
  }

  function bindFacade(bridge: DesktopBridge): TaskCenterFacade {
    facade = createTaskCenterFacade(bridge);
    return facade;
  }

  async function attach(bridge: DesktopBridge): Promise<void> {
    disposed = false;
    bindFacade(bridge);
    await refresh();
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    if (!facade) return;
    try {
      unsubscribe = await facade.subscribe((evt) => {
        if (disposed) return;
        handleBridgeEvent(evt);
      });
    } catch (error) {
      loadState.value = tasksById.value.size > 0 ? "stale" : "error";
      errorMessage.value =
        error instanceof Error
          ? error.message
          : "无法订阅任务事件";
    }
  }

  function detach(): void {
    disposed = true;
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    facade = null;
  }

  async function refresh(): Promise<void> {
    if (!facade) {
      loadState.value = "error";
      errorMessage.value = "Task Center 未绑定 DesktopBridge";
      return;
    }
    const previousState = loadState.value;
    if (previousState !== "stale" && previousState !== "ready") {
      loadState.value = "loading";
    }
    const result = await facade.listTasks();
    if (disposed) return;

    if (result.bridgeError && result.tasks.length === 0 && !result.ready) {
      if (tasksById.value.size > 0) {
        loadState.value = "stale";
        errorMessage.value = result.bridgeError;
      } else {
        loadState.value = "error";
        errorMessage.value = result.bridgeError;
      }
      return;
    }

    // Only accept equal-or-newer list versions.
    if (result.version < version.value) {
      return;
    }

    // Preserve per-task seq/activity when re-listing.
    const prev = tasksById.value;
    const merged = result.tasks.map((task) => {
      const old = prev.get(task.id);
      if (!old) return task;
      return {
        ...task,
        lastSeq: Math.max(old.lastSeq, task.lastSeq),
        phase: task.phase ?? old.phase,
        latestActivity: task.latestActivity ?? old.latestActivity,
        localError: old.localError,
      };
    });
    replaceTasks(merged);
    projects.value = result.projects;
    version.value = result.version;
    refreshedAt.value = result.refreshedAt;
    loadState.value = "ready";
    errorMessage.value = null;
  }

  function markStale(reason?: string): void {
    if (tasksById.value.size > 0) {
      loadState.value = "stale";
    } else {
      loadState.value = "error";
    }
    if (reason) errorMessage.value = reason;
  }

  function handleBridgeEvent(evt: TaskCenterBridgeEvent): void {
    if (evt.kind === "task.snapshot") {
      const seq = evt.event.seq ?? 0;
      if (seq < maxSeq.value) {
        // Older snapshot must not overwrite newer state.
        return;
      }
      maxSeq.value = Math.max(maxSeq.value, seq);
      const parsed = parseSnapshotTasks(
        evt.event.payload,
        projects.value,
        tasksById.value,
      );
      if (parsed.error) {
        errorMessage.value = parsed.error;
        // Keep existing tasks; show soft compatibility notice.
        return;
      }
      if (parsed.tasks) {
        // Merge: snapshot replaces listed tasks but keep unknown locals only if not full replace.
        // Full list from snapshot is authoritative for listed IDs.
        const map = new Map(tasksById.value);
        const seen = new Set<TaskId>();
        for (const task of parsed.tasks) {
          const old = map.get(task.id);
          if (old && old.lastSeq > seq) continue;
          map.set(task.id, {
            ...task,
            lastSeq: Math.max(task.lastSeq, seq),
            phase: task.phase ?? old?.phase,
            latestActivity: task.latestActivity ?? old?.latestActivity,
          });
          seen.add(task.id);
        }
        // When payload provides a full array, drop tasks absent from snapshot only if non-empty list.
        if (parsed.tasks.length > 0) {
          for (const id of Array.from(map.keys())) {
            if (!seen.has(id)) {
              // Keep terminal tasks not in active snapshot? Prefer keep — bootstrap is source for archive.
              // Only update known IDs from event.
            }
          }
        }
        tasksById.value = map;
        if (loadState.value === "stale") loadState.value = "ready";
        announce("任务列表已更新");
      }
      return;
    }

    if (evt.kind === "task.state") {
      const seq = evt.event.seq ?? 0;
      const taskId = evt.event.taskId;
      const existing = tasksById.value.get(taskId);
      if (existing && seq < existing.lastSeq) {
        return;
      }
      maxSeq.value = Math.max(maxSeq.value, seq);
      const statusRaw = evt.event.payload?.status;
      if (typeof statusRaw !== "string" || !isKnownTaskStatus(statusRaw)) {
        if (existing) {
          upsertTask({
            ...existing,
            lastSeq: Math.max(existing.lastSeq, seq),
            localError: "收到未知任务状态，已忽略状态变更",
          });
        }
        return;
      }
      const status: TaskStatus = statusRaw;
      const updatedAt = evt.event.timestamp || new Date().toISOString();
      if (existing) {
        upsertTask({
          ...existing,
          status,
          updatedAt,
          lastSeq: Math.max(existing.lastSeq, seq),
          localError: undefined,
          interruptReason:
            status === "interrupted"
              ? existing.interruptReason
              : existing.interruptReason,
        });
        announce(`任务状态：${status}`);
      } else {
        // Unknown task — wait for list refresh; do not invent title/project.
        void refresh();
      }
      return;
    }

    if (evt.kind === "activity.updated") {
      const seq = evt.event.seq ?? 0;
      const taskId = evt.event.taskId;
      const existing = tasksById.value.get(taskId);
      if (!existing) return;
      if (seq < existing.lastSeq) return;
      maxSeq.value = Math.max(maxSeq.value, seq);
      const detailText =
        typeof evt.event.payload?.detail === "string"
          ? evt.event.payload.detail
          : existing.latestActivity;
      const kind =
        typeof evt.event.payload?.kind === "string"
          ? evt.event.payload.kind
          : existing.phase;
      upsertTask({
        ...existing,
        lastSeq: Math.max(existing.lastSeq, seq),
        latestActivity: detailText,
        phase: kind,
        updatedAt: evt.event.timestamp || existing.updatedAt,
      });
    }
  }

  function setFilters(patch: Partial<TaskCenterFilters>): void {
    filters.value = { ...filters.value, ...patch };
  }

  function resetFilters(): void {
    filters.value = { ...DEFAULT_FILTERS };
  }

  function selectTask(taskId: TaskId | null): void {
    selectedTaskId.value = taskId;
    if (!taskId) {
      detail.value = null;
    }
  }

  async function openDetail(taskId: TaskId): Promise<void> {
    selectedTaskId.value = taskId;
    const task = tasksById.value.get(taskId);
    if (!task) {
      detail.value = null;
      return;
    }
    detailLoading.value = true;
    detail.value = { task };
    if (!facade) {
      detailLoading.value = false;
      return;
    }
    try {
      const result = await facade.getTaskSnapshot(taskId);
      if (disposed || selectedTaskId.value !== taskId) return;
      if (result.success === "false") {
        detail.value = {
          task,
          compatibilityError: result.error.message,
        };
      } else {
        const data = result.data as TaskOpenResult;
        // Enrich display only; never invent domain fields not returned.
        if (!data || typeof data !== "object" || !("taskId" in data)) {
          detail.value = {
            task,
            compatibilityError: "任务详情响应缺少必要字段",
          };
        } else {
          detail.value = {
            task,
            openTitle: typeof data.title === "string" ? data.title : undefined,
            openStatus: typeof data.status === "string" ? data.status : undefined,
          };
        }
      }
    } catch (error) {
      if (disposed || selectedTaskId.value !== taskId) return;
      detail.value = {
        task,
        compatibilityError:
          error instanceof Error ? error.message : String(error),
      };
    } finally {
      detailLoading.value = false;
    }
  }

  function closeDetail(): void {
    selectedTaskId.value = null;
    detail.value = null;
  }

  /**
   * Cancel waits for backend confirmation — no optimistic status flip.
   */
  async function cancelTask(taskId: TaskId): Promise<{ ok: boolean; message?: string }> {
    if (!facade) return { ok: false, message: "未绑定 Bridge" };
    const task = tasksById.value.get(taskId);
    if (!task) return { ok: false, message: "任务不存在" };
    cancelPendingId.value = taskId;
    try {
      const result = await facade.cancelTask(taskId);
      if (result.success === "false") {
        const msg = result.error.message;
        // Terminal / harmless: surface message, do not invent status.
        announce(msg, true);
        return { ok: false, message: msg };
      }
      announce("已请求取消任务", true);
      return { ok: true };
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      announce(msg, true);
      return { ok: false, message: msg };
    } finally {
      cancelPendingId.value = null;
    }
  }

  /** Test helper: inject tasks without bridge. */
  function __setTasksForTest(tasks: TaskViewModel[], nextVersion = 1): void {
    replaceTasks(tasks);
    version.value = nextVersion;
    loadState.value = "ready";
    refreshedAt.value = new Date().toISOString();
  }

  function __setFacadeForTest(next: TaskCenterFacade): void {
    facade = next;
  }

  return {
    loadState,
    errorMessage,
    tasksById,
    projects,
    version,
    maxSeq,
    refreshedAt,
    filters,
    selectedTaskId,
    detail,
    detailLoading,
    cancelPendingId,
    liveMessage,
    allTasks,
    visibleTasks,
    groups,
    counts,
    selectedTask,
    projectOptions,
    attach,
    detach,
    refresh,
    markStale,
    setFilters,
    resetFilters,
    selectTask,
    openDetail,
    closeDetail,
    cancelTask,
    handleBridgeEvent,
    __setTasksForTest,
    __setFacadeForTest,
  };
});
