<script setup lang="ts">
import { provide } from "vue";
import { createFakeDesktopBridge } from "../../bridge/fake-bridge";
import type { DesktopBridge, TaskOpenResult } from "../../bridge/types";
import { createTaskCenterSeedSnapshot } from "./seed";
import TaskCenterView from "./TaskCenterView.vue";

/**
 * DEV / E2E fixture: Task Center with in-memory fake bridge and seed tasks.
 * Opened via `#task-center` or `#task-center/<taskId>` without Tauri host.
 */
const snapshot = createTaskCenterSeedSnapshot();

const bridge: DesktopBridge = createFakeDesktopBridge({
  bootstrapSnapshot: snapshot,
  onExecute(command) {
    if (command.type === "task.open") {
      const task = snapshot.activeTasks?.find((t) => t.id === command.payload.taskId);
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
      const task = snapshot.activeTasks?.find((t) => t.id === command.payload.taskId);
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
      // Simulate backend confirmation via event is test-driven; acknowledge only.
      return { success: "true", data: { acknowledged: "turn.cancel" } };
    }
    return { success: "true", data: { acknowledged: command.type } };
  },
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
