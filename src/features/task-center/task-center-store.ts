// GAG-007: Pinia store for Task Center — snapshot merge, stale, filters.

import { computed, ref, shallowRef } from "vue";
import { defineStore } from "pinia";
import type {
  DesktopBridge,
  ModelInfo,
  Project,
  ProjectId,
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
import { isKnownTaskStatus, presentTaskStatus } from "./status-map";
import { deriveTaskTitle } from "./title";
import {
  DEFAULT_FILTERS,
  type TaskCenterFilters,
  type TaskCenterLoadState,
  type TaskDetailViewModel,
  type TaskViewModel,
} from "./types";

const LIVE_ANNOUNCE_THROTTLE_MS = 1500;
const ACTIVE_PROJECT_KEY = "gag007:activeProjectId";

function classifyProjectError(
  msg: string,
): "invalid" | "non_git" | "failed" {
  if (
    /not found|does not exist|不存在|invalid|无效|ENOENT|not a directory|not accessible|无法访问/i.test(
      msg,
    )
  ) {
    return "invalid";
  }
  if (/not a git|非 git|no git|not a repository/i.test(msg)) {
    return "non_git";
  }
  return "failed";
}

function loadActiveProjectId(): string | null {
  if (typeof sessionStorage === "undefined") return null;
  try {
    return sessionStorage.getItem(ACTIVE_PROJECT_KEY);
  } catch {
    return null;
  }
}

function persistActiveProjectId(id: string | null): void {
  if (typeof sessionStorage === "undefined") return;
  try {
    if (!id) sessionStorage.removeItem(ACTIVE_PROJECT_KEY);
    else sessionStorage.setItem(ACTIVE_PROJECT_KEY, id);
  } catch {
    // ignore quota / private mode
  }
}

export const useTaskCenterStore = defineStore("task-center", () => {
  const loadState = ref<TaskCenterLoadState>("idle");
  const errorMessage = ref<string | null>(null);
  const tasksById = shallowRef<Map<TaskId, TaskViewModel>>(new Map());
  const projects = shallowRef<Project[]>([]);
  const models = shallowRef<ModelInfo[]>([]);
  /** Currently selected project for create-task / shell header. */
  const activeProjectId = ref<ProjectId | null>(null);
  const projectActionError = ref<string | null>(null);
  const projectActionPending = ref(false);
  const createTaskPending = ref(false);
  const createTaskError = ref<string | null>(null);
  /** Highest applied list generation (from facade stamps). */
  const version = ref(0);
  const refreshedAt = ref<string | null>(null);
  const filters = ref<TaskCenterFilters>({ ...DEFAULT_FILTERS });
  const selectedTaskId = ref<TaskId | null>(null);
  /**
   * Overlays from task.open only (title/status strings, compatibility errors).
   * Live task body is always read from tasksById via `detail` computed.
   */
  const detailOverlays = ref<{
    openTitle?: string;
    openStatus?: string;
    compatibilityError?: string;
  } | null>(null);
  const detailLoading = ref(false);
  const cancelPendingId = ref<TaskId | null>(null);
  const liveMessage = ref("");
  let lastAnnounceAt = 0;
  let facade: TaskCenterFacade | null = null;
  let unsubscribe: (() => void) | null = null;
  let disposed = false;
  /** Latest openDetail request id — prevents finally races. */
  let detailRequestId = 0;

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

  /** Live detail: task always from store map so task.state/activity refresh the drawer. */
  const detail = computed<TaskDetailViewModel | null>(() => {
    if (!selectedTaskId.value) return null;
    const task = tasksById.value.get(selectedTaskId.value);
    if (!task) return null;
    return {
      task,
      openTitle: detailOverlays.value?.openTitle,
      openStatus: detailOverlays.value?.openStatus,
      compatibilityError: detailOverlays.value?.compatibilityError,
    };
  });

  const projectOptions = computed(() =>
    projects.value.map((p) => ({
      value: p.id as string,
      label: p.displayPath || p.path || p.id,
    })),
  );

  const modelOptions = computed(() => [
    { value: "", label: "使用运行时默认模型" },
    ...models.value
      .filter((model) => model.modelId.trim().length > 0)
      .map((model) => ({
        value: model.modelId,
        label: model.name || model.modelId,
        ...(model.reasoningEffort
          ? { reasoningEffort: model.reasoningEffort }
          : {}),
      })),
  ]);

  const activeProject = computed(() => {
    if (!activeProjectId.value) return null;
    return projects.value.find((p) => p.id === activeProjectId.value) ?? null;
  });

  const hasActiveProject = computed(() => activeProject.value != null);

  function resolveActiveProject(list: Project[]): void {
    if (list.length === 0) {
      activeProjectId.value = null;
      persistActiveProjectId(null);
      return;
    }
    const remembered = loadActiveProjectId();
    if (remembered && list.some((p) => p.id === remembered)) {
      activeProjectId.value = remembered as ProjectId;
      return;
    }
    // Prefer most recently opened
    const sorted = [...list].sort((a, b) =>
      (b.lastOpenedAt || "").localeCompare(a.lastOpenedAt || ""),
    );
    activeProjectId.value = sorted[0].id;
    persistActiveProjectId(sorted[0].id);
  }

  function setActiveProject(projectId: ProjectId | null): void {
    activeProjectId.value = projectId;
    persistActiveProjectId(projectId);
  }

  function clearActiveProject(): void {
    setActiveProject(null);
    projectActionError.value = null;
    announce("已取消选择项目", true);
  }

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
    // New facade starts its generation at 0; reset so first list applies.
    version.value = 0;
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
      markStale(
        error instanceof Error ? error.message : "无法订阅任务事件",
      );
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

    // Concurrent older response: generation stamped before await.
    if (result.version < version.value) {
      return;
    }

    if (result.bridgeError && result.tasks.length === 0 && !result.ready) {
      if (tasksById.value.size > 0) {
        markStale(result.bridgeError);
      } else {
        loadState.value = "error";
        errorMessage.value = result.bridgeError;
      }
      // Still record generation so we do not re-apply older successes incorrectly.
      version.value = result.version;
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
    models.value = result.models;
    resolveActiveProject(result.projects);
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
    if (evt.kind === "runtime.updated") {
      const status = evt.event.payload?.status;
      if (
        typeof status === "string" &&
        status !== "ready" &&
        status !== "probing"
      ) {
        markStale(`运行时状态：${status}`);
      }
      return;
    }

    if (evt.kind === "task.snapshot") {
      // Session-scoped seq is not globally comparable across tasks/sessions.
      // Each task is guarded by its own lastSeq below.
      const envelopeSeq = evt.event.seq ?? 0;
      const envelopeTaskId = evt.event.taskId;
      const parsed = parseSnapshotTasks(
        evt.event.payload,
        projects.value,
        tasksById.value,
      );
      if (parsed.error) {
        errorMessage.value = parsed.error;
        return;
      }
      if (parsed.tasks) {
        const map = new Map(tasksById.value);
        for (const task of parsed.tasks) {
          const old = map.get(task.id);
          // Per-task seq guard: drop stale updates for this task only.
          if (old && old.lastSeq > envelopeSeq) continue;
          // If snapshot is scoped to one taskId and this row is unrelated, still merge listed IDs.
          if (
            envelopeTaskId &&
            parsed.tasks.length === 1 &&
            task.id !== envelopeTaskId &&
            old &&
            old.lastSeq > envelopeSeq
          ) {
            continue;
          }
          map.set(task.id, {
            ...task,
            lastSeq: Math.max(task.lastSeq, envelopeSeq),
            phase: task.phase ?? old?.phase,
            latestActivity: task.latestActivity ?? old?.latestActivity,
          });
        }
        // Retention: only update IDs present in the snapshot payload; keep others.
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
        const label = presentTaskStatus(status).label;
        announce(`任务「${existing.title}」：${label}`);
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
      detailOverlays.value = null;
    }
  }

  async function openDetail(taskId: TaskId): Promise<void> {
    const requestId = ++detailRequestId;
    selectedTaskId.value = taskId;
    const task = tasksById.value.get(taskId);
    if (!task) {
      detailOverlays.value = null;
      if (requestId === detailRequestId) detailLoading.value = false;
      return;
    }
    detailLoading.value = true;
    // Clear prior overlays; live task is visible via `detail` computed immediately.
    detailOverlays.value = {};
    if (!facade) {
      if (requestId === detailRequestId) detailLoading.value = false;
      return;
    }
    try {
      const result = await facade.getTaskSnapshot(taskId);
      if (disposed || requestId !== detailRequestId || selectedTaskId.value !== taskId) {
        return;
      }
      if (result.success === "false") {
        detailOverlays.value = {
          compatibilityError: result.error.message,
        };
      } else {
        const data = result.data as TaskOpenResult;
        if (!data || typeof data !== "object" || !("taskId" in data)) {
          detailOverlays.value = {
            compatibilityError: "任务详情响应缺少必要字段",
          };
        } else {
          detailOverlays.value = {
            openTitle: typeof data.title === "string" ? data.title : undefined,
            openStatus: typeof data.status === "string" ? data.status : undefined,
          };
        }
      }
    } catch (error) {
      if (disposed || requestId !== detailRequestId || selectedTaskId.value !== taskId) {
        return;
      }
      detailOverlays.value = {
        compatibilityError:
          error instanceof Error ? error.message : String(error),
      };
    } finally {
      // Only the latest in-flight open owns detailLoading.
      if (requestId === detailRequestId) {
        detailLoading.value = false;
      }
    }
  }

  function closeDetail(): void {
    detailRequestId += 1;
    selectedTaskId.value = null;
    detailOverlays.value = null;
    detailLoading.value = false;
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

  /**
   * Open a project path: optional pre-inspect, then project.open, then refresh.
   * Returns structured outcome for dialog UX.
   */
  async function openProjectPath(
    path: string,
  ): Promise<{
    ok: boolean;
    message?: string;
    code?: "cancelled" | "invalid" | "non_git" | "failed" | "ok";
    project?: Project;
  }> {
    const trimmed = path.trim();
    if (!trimmed) {
      projectActionError.value = "请输入或选择项目目录";
      return { ok: false, code: "invalid", message: projectActionError.value };
    }
    if (!facade) {
      projectActionError.value = "未绑定 DesktopBridge";
      return { ok: false, code: "failed", message: projectActionError.value };
    }

    projectActionPending.value = true;
    projectActionError.value = null;
    try {
      // Preflight inspect when available (invalid / non-git messaging).
      const inspect = await facade.inspectWorkspace(trimmed);
      if (inspect.success === "false") {
        const msg = inspect.error.message || "无法打开该目录";
        projectActionError.value = msg;
        const code = classifyProjectError(msg);
        // Hard-fail invalid paths; non_git may still open
        if (code === "invalid" || code === "failed") {
          return { ok: false, code, message: msg };
        }
      }

      const opened = await facade.openProject(trimmed);
      if (opened.success === "false") {
        const msg = opened.error.message || "打开项目失败";
        projectActionError.value = msg;
        return { ok: false, code: classifyProjectError(msg), message: msg };
      }

      await refresh();
      const id = opened.data.projectId;
      setActiveProject(id);
      // Ensure project is in list even if bootstrap race
      if (!projects.value.some((p) => p.id === id)) {
        const stub: Project = {
          id,
          path: opened.data.path ?? trimmed,
          displayPath: opened.data.displayPath ?? trimmed,
          repoRoot: opened.data.repoRoot,
          lastOpenedAt: new Date().toISOString(),
        };
        projects.value = [stub, ...projects.value];
      }
      announce(`已打开项目 ${opened.data.displayPath ?? trimmed}`, true);
      return {
        ok: true,
        code: opened.data.nonGit ? "non_git" : "ok",
        project: activeProject.value ?? undefined,
        message: opened.data.nonGit
          ? "已打开（无 Git：Worktree/集成功能将隐藏）"
          : undefined,
      };
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      projectActionError.value = msg;
      return { ok: false, code: "failed", message: msg };
    } finally {
      projectActionPending.value = false;
    }
  }

  async function createTask(input: {
    prompt: string;
    title?: string;
    mode?: string;
    model?: string;
    reasoning?: import("../../bridge/types").ReasoningEffort;
    workspaceStrategy?: "worktree" | "readonly" | "direct";
  }): Promise<{ ok: boolean; taskId?: TaskId; message?: string }> {
    createTaskError.value = null;
    if (!facade) {
      createTaskError.value = "未绑定 DesktopBridge";
      return { ok: false, message: createTaskError.value };
    }
    if (!activeProjectId.value) {
      createTaskError.value = "请先选择项目";
      return { ok: false, message: createTaskError.value };
    }
    const prompt = input.prompt.trim();
    if (!prompt) {
      createTaskError.value = "请填写任务目标（必填）";
      return { ok: false, message: createTaskError.value };
    }
    // Title is optional: derive it from the first sentence of the prompt.
    const title = (input.title?.trim() || deriveTaskTitle(prompt)).slice(
      0,
      120,
    );

    createTaskPending.value = true;
    try {
      const result = await facade.createTask({
        projectId: activeProjectId.value,
        title,
        prompt,
        mode: input.mode,
        model: input.model,
        reasoning: input.reasoning,
        workspaceStrategy: input.workspaceStrategy,
      });
      if (result.success === "false") {
        createTaskError.value = result.error.message || "创建任务失败";
        return { ok: false, message: createTaskError.value };
      }
      await refresh();
      announce(`已创建任务「${title}」`, true);
      return { ok: true, taskId: result.data.taskId };
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      createTaskError.value = msg;
      return { ok: false, message: msg };
    } finally {
      createTaskPending.value = false;
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

  function __setProjectsForTest(list: Project[], activeId?: ProjectId | null): void {
    projects.value = list;
    if (activeId !== undefined) {
      setActiveProject(activeId);
    } else {
      resolveActiveProject(list);
    }
  }

  return {
    loadState,
    errorMessage,
    tasksById,
    projects,
    models,
    activeProjectId,
    activeProject,
    hasActiveProject,
    projectActionError,
    projectActionPending,
    createTaskPending,
    createTaskError,
    version,
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
    modelOptions,
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
    setActiveProject,
    clearActiveProject,
    openProjectPath,
    createTask,
    handleBridgeEvent,
    announce,
    __setTasksForTest,
    __setFacadeForTest,
    __setProjectsForTest,
  };
});
