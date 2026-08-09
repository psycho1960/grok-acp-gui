<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { createFakeDesktopBridge } from "../../bridge/fake-bridge";
import type { TaskId, TypedDesktopEvent } from "../../bridge/types";
import {
  FIX_TASK,
  fixtureConversationEvents,
  fixtureSessionSnapshot,
  generateManyEvents,
} from "./fixtures";
import { createConversationSeedSnapshot } from "./seed";
import ConversationView from "./ConversationView.vue";

const props = withDefaults(
  defineProps<{
    /** When true, auto-play remaining live events after snapshot. */
    autoPlay?: boolean;
    /** Generate N bulk events for perf smoke (0 = fixture path). */
    bulkEvents?: number;
  }>(),
  {
    autoPlay: true,
    bulkEvents: 0,
  },
);

const conversationEvents = fixtureConversationEvents();

// Persist the per-task selection across reloads (the real backend persists
// it in SQLite; localStorage simulates that for the browser fixture).
const MODE_STORAGE_KEY = "gag010:fixture-mode";
const WORKSPACE_STORAGE_KEY = "gag010:fixture-workspace";
const MODEL_STORAGE_KEY = "gag010:fixture-model";
const REASONING_STORAGE_KEY = "gag010:fixture-reasoning";
function readStoredSelection(key: string, fallback: string | null): string | null {
  try {
    const raw = window.localStorage.getItem(key);
    if (raw === null) return fallback;
    return raw === "" ? null : raw;
  } catch {
    return fallback;
  }
}
function writeStoredSelection(key: string, value: string | null): void {
  try {
    window.localStorage.setItem(key, value ?? "");
  } catch {
    // ignore quota / private mode
  }
}

const snapshot = props.bulkEvents > 0
  ? fixtureSessionSnapshot({
      cursor: 0,
      events: [],
      items: [],
      status: "running",
      title: `Perf ${props.bulkEvents}`,
    })
  : fixtureSessionSnapshot({
      mode: readStoredSelection(MODE_STORAGE_KEY, "agent"),
      workspaceStrategy: readStoredSelection(WORKSPACE_STORAGE_KEY, "worktree"),
      model: readStoredSelection(MODEL_STORAGE_KEY, "grok-4.5"),
      reasoning: readStoredSelection(REASONING_STORAGE_KEY, "high"),
    });

const sentMessages: string[] = [];
let cancelCount = 0;
let interactiveSeq = snapshot.cursor;
let configuredMode: string | null = snapshot.mode ?? null;
let configuredWorkspace: string | null = snapshot.workspaceStrategy ?? null;
let configuredModel: string | null = snapshot.model ?? null;
let configuredReasoning: string | null = snapshot.reasoning ?? null;
let blobImportCount = 0;

function nextInteractiveSeq(): number {
  interactiveSeq += 1;
  return interactiveSeq;
}

const bridge = createFakeDesktopBridge({
  bootstrapSnapshot: createConversationSeedSnapshot(),
  onExecute(command) {
    if (command.type === "session.configure") {
      const settings = command.payload.settings ?? {};
      if (typeof settings.mode === "string") {
        configuredMode = settings.mode;
        writeStoredSelection(MODE_STORAGE_KEY, configuredMode);
      }
      if (typeof settings.workspaceStrategy === "string") {
        configuredWorkspace = settings.workspaceStrategy;
        writeStoredSelection(WORKSPACE_STORAGE_KEY, configuredWorkspace);
      }
      if (typeof settings.model === "string") {
        configuredModel = settings.model;
        writeStoredSelection(MODEL_STORAGE_KEY, configuredModel);
      }
      if (typeof settings.reasoning === "string") {
        configuredReasoning = settings.reasoning;
        writeStoredSelection(REASONING_STORAGE_KEY, configuredReasoning);
      }
      return {
        success: "true",
        data: {
          taskId: command.payload.taskId,
          mode: configuredMode,
          workspaceStrategy: configuredWorkspace,
          model: configuredModel,
          reasoning: configuredReasoning,
        },
      };
    }
    if (command.type === "artifact.import.blob") {
      blobImportCount += 1;
      const blobs = Array.isArray(command.payload.blobs) ? command.payload.blobs : [];
      return {
        success: "true",
        data: {
          artifacts: blobs.map((blob: { displayName?: string }, index: number) => ({
            artifactId: `artifact-clip-${blobImportCount}-${index}`,
            displayName: blob.displayName ?? "剪贴板图片.png",
            mimeType: "image/png",
            bytes: 1024,
            state: "ready",
            previewCapability: "inline",
          })),
        },
      };
    }
    if (command.type === "turn.send") {
      sentMessages.push(command.payload.message);
      // Echo assistant reply for interactive fixture; include the current
      // model/reasoning selection so e2e can observe what a turn carried.
      queueMicrotask(() => {
        push({
          type: "task.state",
          taskId: command.payload.taskId,
          sessionId: "sess-conv-1" as never,
          seq: nextInteractiveSeq(),
          timestamp: new Date().toISOString(),
          payload: {
            taskId: command.payload.taskId,
            status: "running",
            detail: null,
          },
        });
        push({
          type: "message.delta",
          taskId: command.payload.taskId,
          sessionId: "sess-conv-1" as never,
          seq: nextInteractiveSeq(),
          timestamp: new Date().toISOString(),
          payload: {
            text: `回复：${command.payload.message} [mode=${configuredMode ?? "-"} workspace=${configuredWorkspace ?? "-"} model=${configuredModel ?? "-"} reasoning=${configuredReasoning ?? "-"}]`,
          },
        });
        push({
          type: "task.state",
          taskId: command.payload.taskId,
          sessionId: "sess-conv-1" as never,
          seq: nextInteractiveSeq(),
          timestamp: new Date().toISOString(),
          payload: {
            taskId: command.payload.taskId,
            status: "merged",
            detail: null,
          },
        });
      });
      return { success: "true", data: { seq: sentMessages.length } };
    }
    if (command.type === "turn.cancel") {
      cancelCount += 1;
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
      queueMicrotask(() => {
        push({
          type: "task.state",
          taskId: command.payload.taskId,
          sessionId: "sess-conv-1" as never,
          seq: nextInteractiveSeq(),
          timestamp: new Date().toISOString(),
          payload: {
            taskId: command.payload.taskId,
            status: "idle",
            detail: { reason: "cancelled" },
          },
        });
      });
      return { success: "true", data: { acknowledged: "turn.cancel" } };
    }
    if (command.type === "task.open") {
      return {
        success: "true",
        data: {
          taskId: command.payload.taskId,
          title: "对话演示",
          status: "running",
          mode: configuredMode,
          workspaceStrategy: configuredWorkspace,
          model: configuredModel,
          reasoning: configuredReasoning,
        },
      };
    }
    return { success: "true", data: { acknowledged: command.type } };
  },
});

const taskId = FIX_TASK as TaskId;
let timer: ReturnType<typeof setInterval> | null = null;
const playIndex = ref(0);

function push(event: TypedDesktopEvent): void {
  if ("seq" in event && typeof event.seq === "number") {
    interactiveSeq = Math.max(interactiveSeq, event.seq);
  }
  bridge.pushEvent(event);
}

onMounted(() => {
  if (props.bulkEvents > 0) {
    const events = generateManyEvents(props.bulkEvents);
    // Apply in chunks so UI stays responsive
    let i = 0;
    timer = setInterval(() => {
      const chunk = events.slice(i, i + 200);
      for (const e of chunk) push(e);
      i += 200;
      if (i >= events.length && timer) {
        clearInterval(timer);
        timer = null;
      }
    }, 0);
    return;
  }

  if (!props.autoPlay) return;
  const live = conversationEvents.filter((e) => {
    if (!("seq" in e) || e.seq == null) return true;
    return e.seq > snapshot.cursor;
  });
  timer = setInterval(() => {
    if (playIndex.value >= live.length) {
      if (timer) clearInterval(timer);
      timer = null;
      return;
    }
    push(live[playIndex.value]);
    playIndex.value += 1;
  }, 80);
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
});

defineExpose({ bridge, sentMessages, cancelCount: () => cancelCount });
</script>

<template>
  <div class="fixture-root" data-testid="conversation-fixture">
    <ConversationView
      :bridge="bridge"
      :task-id="taskId"
      :snapshot="snapshot"
    />
  </div>
</template>

<style scoped>
.fixture-root {
  height: 100vh;
  min-height: 480px;
}
</style>
