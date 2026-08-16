// GAG-007: Feature-local facade mapping Task Center semantics onto DesktopBridge.
// Does NOT change bridge contracts.

import type {
  BootstrapSnapshot,
  DesktopBridge,
  DesktopResult,
  ModelInfo,
  Project,
  ReasoningEffort,
  SessionBinding,
  Task,
  TaskId,
  TaskOpenResult,
  TypedDesktopEvent,
  Unsubscribe,
  WorktreeRecord,
  WorkspaceKind,
} from "../../bridge/types";
import type { TaskViewModel } from "./types";
import { isKnownTaskStatus } from "./status-map";

export interface ListTasksResult {
  tasks: TaskViewModel[];
  projects: Project[];
  /** Model choices supplied by the runtime capability snapshot. */
  models: ModelInfo[];
  /**
   * Monotonic list generation stamped *before* bootstrap await so concurrent
   * callers cannot let a slower older response win by finishing last.
   */
  version: number;
  refreshedAt: string;
  ready: boolean;
  bridgeError?: string;
}

export type TaskCenterBridgeEvent =
  | { kind: "task.snapshot"; event: Extract<TypedDesktopEvent, { type: "task.snapshot" }> }
  | { kind: "task.state"; event: Extract<TypedDesktopEvent, { type: "task.state" }> }
  | { kind: "activity.updated"; event: Extract<TypedDesktopEvent, { type: "activity.updated" }> }
  | { kind: "runtime.updated"; event: Extract<TypedDesktopEvent, { type: "runtime.updated" }> };

export interface OpenProjectResult {
  projectId: import("../../bridge/types").ProjectId;
  path?: string;
  displayPath?: string;
  repoRoot?: string;
  /** When true, directory is usable but not a git repo. */
  nonGit?: boolean;
}

export interface CreateTaskResult {
  taskId: TaskId;
  title?: string;
  status?: string;
}

export interface TaskCenterFacade {
  listTasks(): Promise<ListTasksResult>;
  getTaskSnapshot(taskId: TaskId): Promise<DesktopResult<TaskOpenResult>>;
  cancelTask(taskId: TaskId): Promise<DesktopResult>;
  /** Validate path (existence / git) without persisting. */
  inspectWorkspace(path: string): Promise<DesktopResult<import("../../bridge/types").WorkspaceInspectResult>>;
  /** Open or re-open a project directory and persist it. */
  openProject(path: string): Promise<DesktopResult<OpenProjectResult>>;
  createTask(input: {
    projectId: import("../../bridge/types").ProjectId;
    title: string;
    prompt: string;
    mode?: string;
    model?: string;
    reasoning?: ReasoningEffort;
    workspaceStrategy?: "worktree" | "readonly" | "direct";
  }): Promise<DesktopResult<CreateTaskResult>>;
  subscribe(listener: (event: TaskCenterBridgeEvent) => void): Promise<Unsubscribe>;
}

function projectLabel(projects: readonly Project[], projectId: string): string {
  const match = projects.find((p) => p.id === projectId);
  if (!match) return projectId;
  return match.displayPath || match.path || projectId;
}

function bindingFor(
  bindings: readonly SessionBinding[] | undefined,
  taskId: TaskId,
): SessionBinding | undefined {
  return bindings?.find((b) => b.taskId === taskId);
}

function worktreeFor(
  worktrees: readonly WorktreeRecord[] | undefined,
  taskId: TaskId,
): WorktreeRecord | undefined {
  return worktrees?.find((w) => w.taskId === taskId);
}

function isWorkspaceKind(value: unknown): value is WorkspaceKind {
  return value === "worktree" || value === "readonly" || value === "direct";
}

/** Build a view model from a Task + bootstrap context. Never invents status. */
export function toTaskViewModel(
  task: Task,
  projects: readonly Project[],
  bindings?: readonly SessionBinding[],
  worktrees?: readonly WorktreeRecord[],
  extras?: Partial<Pick<TaskViewModel, "phase" | "latestActivity" | "lastSeq" | "localError">>,
): TaskViewModel {
  const binding = bindingFor(bindings, task.id);
  const worktree = worktreeFor(worktrees, task.id);
  return {
    id: task.id,
    projectId: task.projectId,
    projectLabel: projectLabel(projects, task.projectId),
    title: task.title.trim() || "新任务",
    status: task.status,
    workspaceKind: task.workspaceKind,
    mode: task.mode,
    model: task.model,
    createdAt: task.createdAt,
    updatedAt: task.updatedAt,
    interruptReason: task.interruptReason,
    hasLiveSession: binding?.state === "active" || binding?.state === "idle",
    sessionId: binding?.sessionId,
    sessionState: binding?.state,
    worktreeDisplayPath: worktree?.displayPath,
    branch: worktree?.branch,
    baseBranch: worktree?.baseBranch,
    worktreeState: worktree?.state,
    lastSeq: extras?.lastSeq ?? binding?.lastSeq ?? 0,
    phase: extras?.phase,
    latestActivity: extras?.latestActivity,
    localError: extras?.localError,
  };
}

export function mapBootstrapToTasks(snapshot: BootstrapSnapshot): TaskViewModel[] {
  const projects = snapshot.projects ?? [];
  const tasks = [
    ...(snapshot.activeTasks ?? []),
    ...(snapshot.completedTasks ?? []),
  ];
  return tasks.map((task) =>
    toTaskViewModel(task, projects, snapshot.bindings, snapshot.worktrees),
  );
}

/**
 * Parse task.snapshot payload. Missing/invalid shape → compatibility error,
 * never invent domain state. Incomplete items without prior state are skipped
 * with localError only when a previous task can host the notice.
 */
export function parseSnapshotTasks(
  payload: unknown,
  projects: readonly Project[],
  previous: ReadonlyMap<TaskId, TaskViewModel>,
): { tasks?: TaskViewModel[]; error?: string } {
  if (payload == null || typeof payload !== "object") {
    return { error: "task.snapshot 缺少有效 payload" };
  }
  const record = payload as Record<string, unknown>;
  const rawTasks = record.tasks;
  if (!Array.isArray(rawTasks)) {
    if (rawTasks == null) return { tasks: [] };
    return { error: "task.snapshot.tasks 不是数组" };
  }

  const out: TaskViewModel[] = [];
  for (const item of rawTasks) {
    if (!item || typeof item !== "object") continue;
    const t = item as Record<string, unknown>;
    if (typeof t.id !== "string" || typeof t.projectId !== "string") {
      continue;
    }
    const id = t.id as TaskId;
    const prev = previous.get(id);

    if (typeof t.status !== "string" || !isKnownTaskStatus(t.status)) {
      if (prev) {
        out.push({
          ...prev,
          localError: "任务状态字段不兼容，已保留上次已知状态",
        });
      }
      // No prior state and unknown status: skip rather than invent.
      continue;
    }

    // Required fields: title, workspaceKind, createdAt, updatedAt — only from payload or prior.
    const rawTitle = typeof t.title === "string" ? t.title : prev?.title;
    const title = rawTitle === "" ? "新任务" : rawTitle;
    const workspaceKind = isWorkspaceKind(t.workspaceKind)
      ? t.workspaceKind
      : prev?.workspaceKind;
    const createdAt = typeof t.createdAt === "string" ? t.createdAt : prev?.createdAt;
    const updatedAt = typeof t.updatedAt === "string" ? t.updatedAt : prev?.updatedAt;

    if (!title || !workspaceKind || !createdAt || !updatedAt) {
      if (prev) {
        out.push({
          ...prev,
          localError: "任务快照字段不完整，已保留上次已知状态",
        });
      }
      // Incomplete without prior: skip — do not invent epoch/title/kind.
      continue;
    }

    const task: Task = {
      id: id as Task["id"],
      projectId: t.projectId as Task["projectId"],
      title,
      status: t.status,
      workspaceKind,
      mode: typeof t.mode === "string" ? t.mode : prev?.mode,
      model: typeof t.model === "string" ? t.model : prev?.model,
      reasoning: typeof t.reasoning === "string" ? t.reasoning : undefined,
      createdAt,
      updatedAt,
      interruptReason:
        typeof t.interruptReason === "string"
          ? t.interruptReason
          : prev?.interruptReason,
    };
    out.push(
      toTaskViewModel(task, projects as Project[], undefined, undefined, {
        phase: prev?.phase,
        latestActivity: prev?.latestActivity,
        lastSeq: prev?.lastSeq ?? 0,
      }),
    );
  }
  return { tasks: out };
}

export function createTaskCenterFacade(bridge: DesktopBridge): TaskCenterFacade {
  let listVersion = 0;

  return {
    async listTasks(): Promise<ListTasksResult> {
      // Stamp generation before I/O so a slower older call cannot win.
      const version = ++listVersion;
      try {
        const snapshot = await bridge.bootstrap();
        return {
          tasks: mapBootstrapToTasks(snapshot),
          projects: snapshot.projects ?? [],
          models: snapshot.capabilities.models ?? [],
          version,
          refreshedAt: new Date().toISOString(),
          ready: snapshot.ready,
          bridgeError: snapshot.dbError,
        };
      } catch (error) {
        return {
          tasks: [],
          projects: [],
          models: [],
          version,
          refreshedAt: new Date().toISOString(),
          ready: false,
          bridgeError:
            error instanceof Error ? error.message : String(error),
        };
      }
    },

    async getTaskSnapshot(taskId: TaskId): Promise<DesktopResult<TaskOpenResult>> {
      const result = await bridge.execute({
        type: "task.open",
        payload: { taskId },
      });
      return result as DesktopResult<TaskOpenResult>;
    },

    async cancelTask(taskId: TaskId): Promise<DesktopResult> {
      return bridge.execute({
        type: "turn.cancel",
        payload: { taskId },
      });
    },

    async inspectWorkspace(path: string) {
      return bridge.execute({
        type: "workspace.inspect",
        payload: { path },
      }) as Promise<
        DesktopResult<import("../../bridge/types").WorkspaceInspectResult>
      >;
    },

    async openProject(path: string) {
      const result = await bridge.execute({
        type: "project.open",
        payload: { path },
      });
      if (result.success === "false") return result;
      const data = result.data as Record<string, unknown> | null;
      const projectId =
        data && typeof data === "object"
          ? (data.projectId as OpenProjectResult["projectId"] | undefined) ??
            (typeof data.id === "string"
              ? (data.id as OpenProjectResult["projectId"])
              : undefined)
          : undefined;
      if (!projectId) {
        return {
          success: "false" as const,
          error: {
            code: "BRIDGE_INVALID_PAYLOAD",
            message: "project.open 响应缺少 projectId",
            retryable: false,
            detailsRedacted: true,
            correlationId: "facade000project" as never,
          },
        };
      }
      return {
        success: "true" as const,
        data: {
          projectId,
          path: typeof data?.path === "string" ? data.path : path,
          displayPath:
            typeof data?.displayPath === "string"
              ? data.displayPath
              : typeof data?.path === "string"
                ? data.path
                : path,
          repoRoot:
            typeof data?.repoRoot === "string" ? data.repoRoot : undefined,
          nonGit: data?.nonGit === true,
        } satisfies OpenProjectResult,
      };
    },

    async createTask(input) {
      const result = await bridge.execute({
        type: "task.create",
        payload: {
          projectId: input.projectId,
          title: input.title,
          prompt: input.prompt,
          mode: input.mode,
          model: input.model,
          reasoning: input.reasoning,
          workspaceStrategy: input.workspaceStrategy,
        },
      });
      if (result.success === "false") return result;
      const data = result.data as Record<string, unknown> | null;
      // Support both { taskId } and { task: { id } } shapes.
      let taskId: TaskId | undefined;
      let title: string | undefined;
      let status: string | undefined;
      if (data && typeof data === "object") {
        if (typeof data.taskId === "string") {
          taskId = data.taskId as TaskId;
        }
        const nested = data.task;
        if (nested && typeof nested === "object") {
          const t = nested as Record<string, unknown>;
          if (typeof t.id === "string") taskId = t.id as TaskId;
          if (typeof t.title === "string") title = t.title;
          if (typeof t.status === "string") status = t.status;
        }
        if (typeof data.title === "string") title = data.title;
        if (typeof data.status === "string") status = data.status;
      }
      if (!taskId) {
        return {
          success: "false" as const,
          error: {
            code: "BRIDGE_INVALID_PAYLOAD",
            message: "task.create 响应缺少 taskId",
            retryable: false,
            detailsRedacted: true,
            correlationId: "facade000taskcreate" as never,
          },
        };
      }
      return {
        success: "true" as const,
        data: { taskId, title, status } satisfies CreateTaskResult,
      };
    },

    async subscribe(
      listener: (event: TaskCenterBridgeEvent) => void,
    ): Promise<Unsubscribe> {
      return bridge.subscribe((event: TypedDesktopEvent) => {
        if (event.type === "task.snapshot") {
          listener({ kind: "task.snapshot", event });
        } else if (event.type === "task.state") {
          listener({ kind: "task.state", event });
        } else if (event.type === "activity.updated") {
          listener({ kind: "activity.updated", event });
        } else if (event.type === "runtime.updated") {
          listener({ kind: "runtime.updated", event });
        }
      });
    },
  };
}
