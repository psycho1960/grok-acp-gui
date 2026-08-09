<script setup lang="ts">
import { onMounted, provide } from "vue";
import { createFakeDesktopBridge } from "../../bridge/fake-bridge";
import type {
  DesktopBridge,
  SessionId,
  TaskId,
  TaskOpenResult,
  TaskStatus,
} from "../../bridge/types";
import { createTaskCenterSeedSnapshot } from "./seed";
import TaskCenterView from "./TaskCenterView.vue";

/**
 * DEV / E2E fixture: Task Center with in-memory fake bridge and seed tasks.
 * Opened via `#task-center` or `#task-center/<taskId>` without Tauri host.
 *
 * Test hooks on window (scoped to fixture route only):
 * - `__taskCenterPushState(taskId, status)` emit task.state
 * - `__taskCenterFailCancel = true` force turn.cancel failures
 */
const snapshot = createTaskCenterSeedSnapshot();

const projects = [...(snapshot.projects ?? [])];
const activeTasks = [...(snapshot.activeTasks ?? [])];
// Keep seed snapshot arrays in sync with local copies for task.open lookups.
snapshot.projects = projects;
snapshot.activeTasks = activeTasks;

const fake = createFakeDesktopBridge({
  bootstrapSnapshot: snapshot,
  onExecute(command) {
    if (command.type === "workspace.inspect") {
      const path = command.payload.path.trim();
      if (!path || /missing/i.test(path)) {
        return {
          success: "false",
          error: {
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Directory does not exist or is not accessible",
            retryable: false,
            detailsRedacted: true,
            correlationId: "fixture000000010" as never,
          },
        };
      }
      return {
        success: "true",
        data: {
          repoRoot: path,
          branch: /nongit/i.test(path) ? "unknown" : "main",
          dirty: false,
        },
      };
    }
    if (command.type === "project.open") {
      const path = command.payload.path.trim();
      if (!path || /missing/i.test(path)) {
        return {
          success: "false",
          error: {
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Directory does not exist or is not accessible",
            retryable: false,
            detailsRedacted: true,
            correlationId: "fixture000000011" as never,
          },
        };
      }
      const nonGit = /nongit/i.test(path);
      let project = projects.find((p) => p.path === path);
      if (!project) {
        project = {
          id: `proj-fixture-${projects.length + 1}` as never,
          path,
          displayPath: path,
          repoRoot: nonGit ? undefined : path,
          lastOpenedAt: new Date().toISOString(),
          trustedAt: new Date().toISOString(),
        };
        projects.unshift(project);
      }
      return {
        success: "true",
        data: {
          projectId: project.id,
          path: project.path,
          displayPath: project.displayPath,
          repoRoot: project.repoRoot,
          nonGit,
        },
      };
    }
    if (command.type === "task.create") {
      if (!command.payload.prompt?.trim()) {
        return {
          success: "false",
          error: {
            code: "BRIDGE_VALIDATION_FAILED",
            message: "Task prompt is required",
            retryable: false,
            detailsRedacted: true,
            correlationId: "fixture000000012" as never,
          },
        };
      }
      const id = `task-new-${activeTasks.length + 1}` as TaskId;
      const now = new Date().toISOString();
      activeTasks.unshift({
        id,
        projectId: command.payload.projectId,
        title: command.payload.title ?? "未命名任务",
        status: "preparing",
        workspaceKind: "worktree",
        mode: command.payload.mode,
        model: command.payload.model,
        createdAt: now,
        updatedAt: now,
      });
      return {
        success: "true",
        data: {
          taskId: id,
          task: { id, title: command.payload.title, status: "preparing" },
        },
      };
    }
    if (command.type === "task.open") {
      const task = activeTasks.find((t) => t.id === command.payload.taskId);
      if (!task) {
        return {
          success: "false",
          error: {
            code: "TEST",
            message: "任务不存在",
            retryable: false,
            detailsRedacted: true,
            correlationId: "fixture000000001" as never,
          },
        };
      }
      const data: TaskOpenResult = {
        taskId: task.id,
        title: task.title,
        status: task.status,
      };
      return { success: "true", data };
    }
    if (command.type === "turn.cancel") {
      const forceFail =
        typeof window !== "undefined" &&
        (window as Window & { __taskCenterFailCancel?: boolean }).__taskCenterFailCancel ===
          true;
      if (forceFail) {
        return {
          success: "false",
          error: {
            code: "TEST",
            message: "取消失败：后端拒绝",
            retryable: true,
            detailsRedacted: true,
            correlationId: "fixture000000004" as never,
          },
        };
      }
      const task = activeTasks.find((t) => t.id === command.payload.taskId);
      if (!task) {
        return {
          success: "false",
          error: {
            code: "TEST",
            message: "任务不存在",
            retryable: false,
            detailsRedacted: true,
            correlationId: "fixture000000002" as never,
          },
        };
      }
      if (
        task.status === "merged" ||
        task.status === "archived" ||
        task.status === "interrupted"
      ) {
        return {
          success: "false",
          error: {
            code: "TEST",
            message: "任务已终态，无法取消",
            retryable: false,
            detailsRedacted: true,
            correlationId: "fixture000000003" as never,
          },
        };
      }
      return { success: "true", data: { acknowledged: "turn.cancel" } };
    }
    return { success: "true", data: { acknowledged: command.type } };
  },
});

const bridge: DesktopBridge = fake;

type FixtureWindow = Window & {
  __taskCenterPushState?: (taskId: string, status: TaskStatus, seq?: number) => void;
  __taskCenterFailCancel?: boolean;
};

onMounted(() => {
  const w = window as FixtureWindow;
  w.__taskCenterPushState = (taskId, status, seq = 100) => {
    const task = activeTasks.find((t) => t.id === taskId);
    if (task) task.status = status;
    fake.pushEvent({
      type: "task.state",
      taskId: taskId as TaskId,
      sessionId: "sess-fixture" as SessionId,
      seq,
      timestamp: new Date().toISOString(),
      payload: {
        taskId: taskId as TaskId,
        status,
        detail: null,
      },
    });
  };
});

provide("desktopBridge", bridge);
</script>

<template>
  <main class="task-center-fixture">
    <TaskCenterView :bridge="bridge" :sync-hash="true" />
  </main>
</template>

<style scoped>
.task-center-fixture {
  min-height: 100vh;
  background: var(--ctp-base);
}
</style>
