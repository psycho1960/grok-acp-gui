import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";
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

  it("ignores older task.state seq and keeps newer snapshot", async () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({
        id: "t1" as TaskId,
        title: "A",
        status: "running",
        lastSeq: 5,
        updatedAt: "2026-04-01T12:00:00.000Z",
      }),
    ]);
    store.maxSeq = 5;

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
    expect(store.tasksById.get("t1" as TaskId)?.lastSeq).toBe(5);
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

    store.handleBridgeEvent({ kind: "task.state", event });
    expect(store.tasksById.get("t1" as TaskId)?.status).toBe("waiting_permission");
    expect(store.tasksById.get("t1" as TaskId)?.lastSeq).toBe(2);
  });

  it("drops older task.snapshot by global maxSeq", () => {
    const store = useTaskCenterStore();
    store.__setTasksForTest([
      vm({ id: "t1" as TaskId, title: "Keep", status: "running", lastSeq: 10 }),
    ]);
    store.maxSeq = 10;

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
    // Status unchanged until task.state event arrives.
    expect(store.tasksById.get(running!.id)?.status).toBe(statusBefore);
  });
});
