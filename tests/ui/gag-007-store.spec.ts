import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type {
  ProjectId,
  SessionId,
  TaskId,
  TypedDesktopEvent,
} from "../../src/bridge/types";
import { createTaskCenterSeedSnapshot } from "../../src/features/task-center/seed";
import { useTaskCenterStore } from "../../src/features/task-center/task-center-store";
import type { TaskViewModel } from "../../src/features/task-center/types";
import type { TaskCenterFacade } from "../../src/features/task-center/task-bridge-facade";

function vm(partial: Partial<TaskViewModel> & Pick<TaskViewModel, "id" | "status" | "title">): TaskViewModel {
  return {
    projectId: "proj-a" as ProjectId,
    projectLabel: "proj-a",
    workspaceKind: "worktree",
    createdAt: "2026-04-01T10:00:00.000Z",
    updatedAt: "2026-04-01T11:00:00.000Z",
    lastSeq: 0,
    ...partial,
  };
}

describe("GAG-007 task-center store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("loads bootstrap snapshot via facade", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    expect(store.loadState).toBe("ready");
    expect(store.allTasks.length).toBe(5);
    expect(store.counts.needs_attention).toBe(1);
    expect(store.counts.running).toBe(2);
    expect(store.counts.completed).toBe(1);
    expect(store.counts.failed_interrupted).toBe(1);
  });

  it("ignores older task.state seq per task (not global)", async () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({
        id: "t1" as TaskId,
        title: "A",
        status: "running",
        lastSeq: 5,
        updatedAt: "2026-04-01T12:00:00.000Z",
      }),
      vm({
        id: "t2" as TaskId,
        title: "B",
        status: "running",
        lastSeq: 1,
        updatedAt: "2026-04-01T12:00:00.000Z",
      }),
    ]);

    // Low-seq event for t1 must not apply.
    store.handleBridgeEvent({
      kind: "task.state",
      event: {
        type: "task.state",
        taskId: "t1" as TaskId,
        sessionId: "s1" as SessionId,
        seq: 3,
        timestamp: "2026-04-01T12:01:00.000Z",
        payload: { taskId: "t1" as TaskId, status: "merged", detail: null },
      },
    });
    expect(store.tasksById.get("t1" as TaskId)?.status).toBe("running");

    // Low-seq relative to global history but newer for t2 must still apply
    // (no global maxSeq drop).
    store.handleBridgeEvent({
      kind: "task.state",
      event: {
        type: "task.state",
        taskId: "t2" as TaskId,
        sessionId: "s2" as SessionId,
        seq: 2,
        timestamp: "2026-04-01T12:01:00.000Z",
        payload: { taskId: "t2" as TaskId, status: "waiting_permission", detail: null },
      },
    });
    expect(store.tasksById.get("t2" as TaskId)?.status).toBe("waiting_permission");
  });

  it("does not drop session-B snapshot after high-seq session-A event", () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({ id: "a" as TaskId, title: "A", status: "running", lastSeq: 50 }),
      vm({ id: "b" as TaskId, title: "B", status: "running", lastSeq: 1 }),
    ]);

    store.handleBridgeEvent({
      kind: "task.snapshot",
      event: {
        type: "task.snapshot",
        taskId: "b" as TaskId,
        sessionId: "sess-b" as SessionId,
        seq: 5,
        timestamp: "2026-04-01T12:00:00.000Z",
        payload: {
          tasks: [
            {
              id: "b",
              projectId: "proj-a",
              title: "B updated",
              status: "merged",
              workspaceKind: "worktree",
              createdAt: "2026-04-01T10:00:00.000Z",
              updatedAt: "2026-04-01T12:00:00.000Z",
            },
          ],
        },
      },
    });

    expect(store.tasksById.get("b" as TaskId)?.status).toBe("merged");
    expect(store.tasksById.get("b" as TaskId)?.title).toBe("B updated");
    expect(store.tasksById.get("a" as TaskId)?.status).toBe("running");
  });

  it("applies newer task.state and is idempotent for same seq", async () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({ id: "t1" as TaskId, title: "A", status: "running", lastSeq: 1 }),
    ]);

    const event: TypedDesktopEvent = {
      type: "task.state",
      taskId: "t1" as TaskId,
      sessionId: "s1" as SessionId,
      seq: 2,
      timestamp: "2026-04-01T12:02:00.000Z",
      payload: { taskId: "t1" as TaskId, status: "waiting_permission", detail: null },
    };

    store.handleBridgeEvent({ kind: "task.state", event });
    expect(store.tasksById.get("t1" as TaskId)?.status).toBe("waiting_permission");
    expect(store.liveMessage).toMatch(/等待审批/);

    store.handleBridgeEvent({ kind: "task.state", event });
    expect(store.tasksById.get("t1" as TaskId)?.status).toBe("waiting_permission");
    expect(store.tasksById.get("t1" as TaskId)?.lastSeq).toBe(2);
  });

  it("drops older per-task snapshot rows by lastSeq", () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({ id: "t1" as TaskId, title: "Keep", status: "running", lastSeq: 10 }),
    ]);

    store.handleBridgeEvent({
      kind: "task.snapshot",
      event: {
        type: "task.snapshot",
        taskId: "t1" as TaskId,
        sessionId: "s1" as SessionId,
        seq: 4,
        timestamp: "2026-04-01T12:00:00.000Z",
        payload: {
          tasks: [
            {
              id: "t1",
              projectId: "proj-a",
              title: "Stale",
              status: "merged",
              workspaceKind: "worktree",
              createdAt: "2026-04-01T10:00:00.000Z",
              updatedAt: "2026-04-01T11:00:00.000Z",
            },
          ],
        },
      },
    });

    expect(store.tasksById.get("t1" as TaskId)?.title).toBe("Keep");
    expect(store.tasksById.get("t1" as TaskId)?.status).toBe("running");
  });

  it("marks stale when refresh fails but previous tasks remain", async () => {
    let fail = false;
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
    });
    const originalBootstrap = bridge.bootstrap.bind(bridge);
    bridge.bootstrap = async () => {
      if (fail) throw new Error("bridge offline");
      return originalBootstrap();
    };

    const store = useTaskCenterStore();
    await store.attach(bridge);
    expect(store.loadState).toBe("ready");
    expect(store.allTasks.length).toBeGreaterThan(0);

    fail = true;
    await store.refresh();
    expect(store.loadState).toBe("stale");
    expect(store.allTasks.length).toBeGreaterThan(0);
    expect(store.errorMessage).toMatch(/offline/i);
  });

  it("marks stale on runtime.updated non-ready", () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({ id: "t1" as TaskId, title: "A", status: "running" }),
    ]);
    store.handleBridgeEvent({
      kind: "runtime.updated",
      event: {
        type: "runtime.updated",
        timestamp: new Date().toISOString(),
        payload: { status: "unavailable" },
      },
    });
    expect(store.loadState).toBe("stale");
  });

  it("cancel waits for bridge result without optimistic status flip", async () => {
    let resolveCancel: ((value: unknown) => void) | null = null;
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
      onExecute(command) {
        if (command.type === "turn.cancel") {
          return new Promise((resolve) => {
            resolveCancel = resolve;
          }) as never;
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const running = store.allTasks.find((t) => t.status === "running");
    expect(running).toBeTruthy();
    const statusBefore = running!.status;

    const pending = store.cancelTask(running!.id);
    expect(store.cancelPendingId).toBe(running!.id);
    expect(store.tasksById.get(running!.id)?.status).toBe(statusBefore);

    resolveCancel?.({ success: "true", data: { acknowledged: "turn.cancel" } });
    const result = await pending;
    expect(result.ok).toBe(true);
    expect(store.tasksById.get(running!.id)?.status).toBe(statusBefore);
  });

  it("cancel failure and throw keep status and clear pending", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
      onExecute(command) {
        if (command.type === "turn.cancel") {
          return {
            success: "false",
            error: {
              code: "TEST",
              message: "cannot cancel",
              retryable: false,
              detailsRedacted: true,
              correlationId: "c" as never,
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const running = store.allTasks.find((t) => t.status === "running")!;
    const result = await store.cancelTask(running.id);
    expect(result.ok).toBe(false);
    expect(result.message).toMatch(/cannot cancel/);
    expect(store.cancelPendingId).toBeNull();
    expect(store.tasksById.get(running.id)?.status).toBe("running");

    // throw path
    const throwBridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
      onExecute(command) {
        if (command.type === "turn.cancel") throw new Error("network down");
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    await store.attach(throwBridge);
    const r2 = store.allTasks.find((t) => t.status === "running")!;
    const thrown = await store.cancelTask(r2.id);
    expect(thrown.ok).toBe(false);
    expect(thrown.message).toMatch(/network down/);
    expect(store.cancelPendingId).toBeNull();
  });

  it("ignores older list version from concurrent/outdated facade results", async () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest(
      [vm({ id: "t1" as TaskId, title: "Current", status: "running" })],
      5,
    );
    const facade: TaskCenterFacade = {
      async listTasks() {
        return {
          tasks: [
            vm({ id: "t1" as TaskId, title: "Stale list", status: "merged" }),
          ],
          projects: [],
          version: 3,
          refreshedAt: new Date().toISOString(),
          ready: true,
        };
      },
      async getTaskSnapshot() {
        return { success: "true", data: { taskId: "t1" as TaskId, title: "x", status: "running" } };
      },
      async cancelTask() {
        return { success: "true", data: {} };
      },
      async subscribe() {
        return () => undefined;
      },
    };
    store.__setFacadeForTest(facade);
    await store.refresh();
    expect(store.tasksById.get("t1" as TaskId)?.title).toBe("Current");
    expect(store.version).toBe(5);
  });

  it("resets version on re-attach so new facade list applies", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const v1 = store.version;
    expect(v1).toBeGreaterThan(0);
    store.detach();
    await store.attach(bridge);
    expect(store.version).toBeGreaterThan(0);
    expect(store.loadState).toBe("ready");
    expect(store.allTasks.length).toBe(5);
  });

  it("detail.task tracks live task.state without re-open", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
      onExecute(command) {
        if (command.type === "task.open") {
          return {
            success: "true",
            data: {
              taskId: command.payload.taskId,
              title: "实现 Task Center UI",
              status: "running",
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    await store.openDetail("task-run-1" as TaskId);
    expect(store.detail?.task.status).toBe("running");
    expect(store.detail?.openTitle).toBe("实现 Task Center UI");

    store.handleBridgeEvent({
      kind: "task.state",
      event: {
        type: "task.state",
        taskId: "task-run-1" as TaskId,
        sessionId: "s" as SessionId,
        seq: 40,
        timestamp: new Date().toISOString(),
        payload: {
          taskId: "task-run-1" as TaskId,
          status: "interrupted",
          detail: null,
        },
      },
    });

    // Live map drives detail.task; open overlay may still show prior openStatus.
    expect(store.detail?.task.status).toBe("interrupted");
    expect(store.detail?.task.id).toBe("task-run-1");
    expect(store.detail?.openTitle).toBe("实现 Task Center UI");
  });

  it("openDetail finally does not clear loading for a newer request", async () => {
    let resolveFirst: ((v: unknown) => void) | null = null;
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
      onExecute(command) {
        if (command.type === "task.open") {
          if (command.payload.taskId === "task-run-1") {
            return new Promise((resolve) => {
              resolveFirst = resolve;
            }) as never;
          }
          return {
            success: "true",
            data: {
              taskId: command.payload.taskId,
              title: "second",
              status: "running",
            },
          };
        }
        return { success: "true", data: { acknowledged: command.type } };
      },
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);

    const first = store.openDetail("task-run-1" as TaskId);
    expect(store.detailLoading).toBe(true);
    const second = store.openDetail("task-wait-1" as TaskId);
    await second;
    expect(store.detailLoading).toBe(false);
    expect(store.selectedTaskId).toBe("task-wait-1");

    resolveFirst?.({
      success: "true",
      data: { taskId: "task-run-1", title: "late", status: "running" },
    });
    await first;
    // Still showing second task; loading stays false.
    expect(store.selectedTaskId).toBe("task-wait-1");
    expect(store.detailLoading).toBe(false);
  });

  it("unknown status sets localError; malformed snapshot keeps prior tasks", () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({ id: "t1" as TaskId, title: "A", status: "running", lastSeq: 1 }),
    ]);

    store.handleBridgeEvent({
      kind: "task.state",
      event: {
        type: "task.state",
        taskId: "t1" as TaskId,
        sessionId: "s1" as SessionId,
        seq: 2,
        timestamp: new Date().toISOString(),
        payload: { taskId: "t1" as TaskId, status: "not_a_real_status", detail: null },
      },
    });
    expect(store.tasksById.get("t1" as TaskId)?.status).toBe("running");
    expect(store.tasksById.get("t1" as TaskId)?.localError).toMatch(/未知任务状态/);

    store.handleBridgeEvent({
      kind: "task.snapshot",
      event: {
        type: "task.snapshot",
        taskId: "t1" as TaskId,
        sessionId: "s1" as SessionId,
        seq: 3,
        timestamp: new Date().toISOString(),
        payload: { tasks: "not-array" },
      },
    });
    expect(store.tasksById.get("t1" as TaskId)?.title).toBe("A");
    expect(store.errorMessage).toMatch(/不是数组/);
  });

  it("does not invent fields for incomplete snapshot without prior", () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([]);
    store.handleBridgeEvent({
      kind: "task.snapshot",
      event: {
        type: "task.snapshot",
        taskId: "new" as TaskId,
        sessionId: "s1" as SessionId,
        seq: 1,
        timestamp: new Date().toISOString(),
        payload: {
          tasks: [{ id: "new", projectId: "p", status: "running" }],
        },
      },
    });
    expect(store.tasksById.has("new" as TaskId)).toBe(false);
  });

  it("merges activity.updated and refreshes on unknown task state", async () => {
    const bridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
    });
    const store = useTaskCenterStore();
    await store.attach(bridge);
    const id = "task-run-1" as TaskId;

    store.handleBridgeEvent({
      kind: "activity.updated",
      event: {
        type: "activity.updated",
        taskId: id,
        sessionId: "s" as SessionId,
        seq: 10,
        timestamp: new Date().toISOString(),
        payload: { kind: "tool", detail: "ran grep" },
      },
    });
    expect(store.tasksById.get(id)?.latestActivity).toBe("ran grep");
    expect(store.tasksById.get(id)?.phase).toBe("tool");

    // Older activity ignored
    store.handleBridgeEvent({
      kind: "activity.updated",
      event: {
        type: "activity.updated",
        taskId: id,
        sessionId: "s" as SessionId,
        seq: 2,
        timestamp: new Date().toISOString(),
        payload: { kind: "old", detail: "stale" },
      },
    });
    expect(store.tasksById.get(id)?.latestActivity).toBe("ran grep");

    let bootstrapCalls = 0;
    const countingBridge = createFakeDesktopBridge({
      bootstrapSnapshot: createTaskCenterSeedSnapshot(),
    });
    const originalBootstrap = countingBridge.bootstrap.bind(countingBridge);
    countingBridge.bootstrap = async () => {
      bootstrapCalls += 1;
      return originalBootstrap();
    };
    await store.attach(countingBridge);
    const callsAfterAttach = bootstrapCalls;
    store.handleBridgeEvent({
      kind: "task.state",
      event: {
        type: "task.state",
        taskId: "unknown-task" as TaskId,
        sessionId: "s" as SessionId,
        seq: 1,
        timestamp: new Date().toISOString(),
        payload: {
          taskId: "unknown-task" as TaskId,
          status: "running",
          detail: null,
        },
      },
    });
    await vi.waitFor(() => {
      expect(bootstrapCalls).toBeGreaterThan(callsAfterAttach);
    });
  });
});
