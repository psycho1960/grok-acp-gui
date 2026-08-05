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
const snapshot = props.bulkEvents > 0
  ? fixtureSessionSnapshot({
      cursor: 0,
      events: [],
      items: [],
      status: "running",
      title: `Perf ${props.bulkEvents}`,
    })
  : fixtureSessionSnapshot();

const sentMessages: string[] = [];
let cancelCount = 0;
let interactiveSeq = snapshot.cursor;

function nextInteractiveSeq(): number {
  interactiveSeq += 1;
  return interactiveSeq;
}

const bridge = createFakeDesktopBridge({
  bootstrapSnapshot: createConversationSeedSnapshot(),
  onExecute(command) {
    if (command.type === "turn.send") {
      sentMessages.push(command.payload.message);
      // Echo assistant reply for interactive fixture
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
          payload: { text: `Echo: ${command.payload.message}` },
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
          title: "Conversation fixture",
          status: "running",
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
