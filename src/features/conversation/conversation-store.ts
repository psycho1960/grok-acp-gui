// GAG-008: Pinia store — snapshot/delta merge, composer, cancel, drafts.

import { computed, ref, shallowRef } from "vue";
import { defineStore } from "pinia";
import type {
  DesktopBridge,
  TaskId,
  TypedDesktopEvent,
} from "../../bridge/types";
import {
  createConversationFacade,
  type ConversationFacade,
} from "./conversation-facade";
import { clearDraft, loadDraft, saveDraft } from "./draft";
import {
  applyEvent,
  applySnapshot,
  appendUserMessage,
  createEmptyConversationState,
  foldExploreTools,
  markUserMessageConfirmed,
  markUserMessageFailed,
  setRunStatus,
  toggleThinkingExpanded,
  toggleToolExpanded,
} from "./reducer";
import type {
  ComposerCapabilities,
  ConversationLoadState,
  ConversationState,
  SessionTimelineSnapshot,
  TimelineItem,
} from "./types";

const DELTA_FLUSH_MS = 32;

export const useConversationStore = defineStore("conversation", () => {
  const loadState = ref<ConversationLoadState>("idle");
  const errorMessage = ref<string | null>(null);
  const timeline = shallowRef<ConversationState>(createEmptyConversationState());
  const draft = ref("");
  const sendError = ref<string | null>(null);
  const sendPending = ref(false);
  const cancelPending = ref(false);
  const bridgeOnline = ref(true);
  const focusEventSeq = ref<number | null>(null);
  /** When true, explore read tools are folded. */
  const foldExplores = ref(true);

  let facade: ConversationFacade | null = null;
  let unsubscribe: (() => void) | null = null;
  let disposed = false;
  let pendingDeltas: TypedDesktopEvent[] = [];
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingUserId: string | null = null;

  const items = computed<TimelineItem[]>(() => {
    const raw = timeline.value.items;
    return foldExplores.value ? foldExploreTools(raw) : raw;
  });

  const status = computed(() => timeline.value.status);
  const title = computed(() => timeline.value.title || "会话");
  const taskId = computed(() => timeline.value.taskId);
  const sessionId = computed(() => timeline.value.sessionId);
  const cursor = computed(() => timeline.value.cursor);
  const needsRefresh = computed(() => timeline.value.needsSnapshotRefresh);
  const attempt = computed(() => timeline.value.attempt);

  const isRunning = computed(
    () =>
      timeline.value.status === "running" ||
      timeline.value.status === "cancelling" ||
      timeline.value.status === "waiting_permission" ||
      timeline.value.status === "waiting_plan",
  );

  const composerCapabilities = computed<ComposerCapabilities>(() => {
    if (!bridgeOnline.value) {
      return {
        canSend: false,
        canCancel: false,
        disabledReason: "Bridge 离线，草稿已保留",
        bridgeOnline: false,
      };
    }
    if (sendPending.value) {
      return {
        canSend: false,
        canCancel: false,
        disabledReason: "正在发送…",
        bridgeOnline: true,
      };
    }
    const st = timeline.value.status;
    if (st === "cancelling") {
      return {
        canSend: false,
        canCancel: false,
        disabledReason: "正在停止…",
        bridgeOnline: true,
      };
    }
    if (st === "disconnected" || st === "offline") {
      return {
        canSend: false,
        canCancel: false,
        disabledReason:
          st === "offline" ? "Bridge 离线，草稿已保留" : "会话已断开",
        bridgeOnline: st !== "offline",
      };
    }
    if (st === "running") {
      return {
        canSend: false,
        canCancel: true,
        disabledReason: "Agent 正在回复，可按 Esc 停止",
        bridgeOnline: true,
      };
    }
    if (st === "waiting_permission") {
      return {
        canSend: false,
        canCancel: true,
        disabledReason: "等待权限审批",
        bridgeOnline: true,
      };
    }
    // idle | error | waiting_plan — allow compose (plan approval is separate slot)
    return {
      canSend: true,
      canCancel: st === "waiting_plan",
      bridgeOnline: true,
    };
  });

  function commit(next: ConversationState): void {
    timeline.value = next;
  }

  function flushDeltas(): void {
    flushTimer = null;
    if (pendingDeltas.length === 0) return;
    const batch = pendingDeltas;
    pendingDeltas = [];
    let next = timeline.value;
    for (const e of batch) {
      next = applyEvent(next, e);
    }
    commit(next);
  }

  function scheduleDelta(event: TypedDesktopEvent): void {
    // High-frequency message.delta batching; other events flush immediately with queue
    if (event.type === "message.delta" && event.payload?.text) {
      pendingDeltas.push(event);
      if (flushTimer == null) {
        flushTimer = setTimeout(flushDeltas, DELTA_FLUSH_MS);
      }
      return;
    }
    // Flush pending text first to preserve character order
    if (pendingDeltas.length > 0) {
      flushDeltas();
    }
    commit(applyEvent(timeline.value, event));
  }

  function handleDesktopEvent(event: TypedDesktopEvent): void {
    if (disposed) return;
    if (event.type === "runtime.updated") {
      if (event.payload.status === "unavailable") {
        bridgeOnline.value = false;
      } else if (event.payload.status === "ready") {
        bridgeOnline.value = true;
      }
    }
    scheduleDelta(event);
  }

  async function attach(bridge: DesktopBridge): Promise<void> {
    disposed = false;
    facade = createConversationFacade(bridge);
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    try {
      unsubscribe = await facade.subscribe((evt) => {
        if (evt.kind === "bridge_error") {
          bridgeOnline.value = false;
          errorMessage.value = evt.message;
          loadState.value = "stale";
          return;
        }
        handleDesktopEvent(evt.event);
      });
      bridgeOnline.value = true;
    } catch (error) {
      bridgeOnline.value = false;
      errorMessage.value =
        error instanceof Error ? error.message : "订阅失败";
      loadState.value = "stale";
    }
  }

  function detach(): void {
    disposed = true;
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    pendingDeltas = [];
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    facade = null;
  }

  function openFromSnapshot(snapshot: SessionTimelineSnapshot): void {
    loadState.value = "loading";
    const next = applySnapshot(createEmptyConversationState(snapshot.taskId), snapshot);
    commit(next);
    draft.value = loadDraft(snapshot.taskId);
    sendError.value = null;
    loadState.value = "ready";
    errorMessage.value = null;
  }

  async function openTask(taskId: TaskId, title = "任务会话"): Promise<void> {
    loadState.value = "loading";
    errorMessage.value = null;
    commit({
      ...createEmptyConversationState(taskId),
      title,
      status: "idle",
    });
    draft.value = loadDraft(taskId);

    if (!facade) {
      loadState.value = "ready";
      return;
    }

    try {
      const result = await facade.openTask(taskId);
      if (result.success === "false") {
        errorMessage.value = result.error.message;
        loadState.value = "error";
        return;
      }
      const data = result.data as { title?: string; status?: string } | undefined;
      if (data?.title) {
        commit({ ...timeline.value, title: data.title });
      }
      loadState.value = "ready";
    } catch (error) {
      errorMessage.value =
        error instanceof Error ? error.message : "打开任务失败";
      loadState.value = "error";
    }
  }

  function setDraft(text: string): void {
    draft.value = text;
    saveDraft(timeline.value.taskId, text);
  }

  async function sendMessage(): Promise<boolean> {
    const text = draft.value.trim();
    if (!text) return false;
    if (!composerCapabilities.value.canSend) return false;
    if (!timeline.value.taskId) {
      sendError.value = "未选择任务";
      return false;
    }
    if (!facade) {
      sendError.value = "Bridge 不可用";
      return false;
    }

    sendPending.value = true;
    sendError.value = null;
    const localId = `user-${Date.now()}`;
    pendingUserId = localId;
    commit(
      appendUserMessage(timeline.value, text, {
        id: localId,
        pending: true,
      }),
    );

    try {
      const result = await facade.sendTurn(timeline.value.taskId, text);
      if (result.success === "false") {
        commit(
          markUserMessageFailed(
            timeline.value,
            localId,
            result.error.message,
          ),
        );
        sendError.value = result.error.message;
        // Keep draft for retry
        return false;
      }
      commit(markUserMessageConfirmed(timeline.value, localId));
      commit(setRunStatus(timeline.value, "running"));
      draft.value = "";
      clearDraft(timeline.value.taskId);
      pendingUserId = null;
      return true;
    } catch (error) {
      const msg = error instanceof Error ? error.message : "发送失败";
      commit(markUserMessageFailed(timeline.value, localId, msg));
      sendError.value = msg;
      return false;
    } finally {
      sendPending.value = false;
    }
  }

  async function cancelTurn(): Promise<boolean> {
    if (!timeline.value.taskId || !facade) return false;
    if (!composerCapabilities.value.canCancel) return false;
    cancelPending.value = true;
    commit(setRunStatus(timeline.value, "cancelling"));
    try {
      const result = await facade.cancelTurn(timeline.value.taskId);
      if (result.success === "false") {
        sendError.value = result.error.message;
        commit(setRunStatus(timeline.value, "running"));
        return false;
      }
      return true;
    } catch (error) {
      sendError.value =
        error instanceof Error ? error.message : "停止失败";
      commit(setRunStatus(timeline.value, "running"));
      return false;
    } finally {
      cancelPending.value = false;
    }
  }

  function toggleTool(itemId: string): void {
    commit(toggleToolExpanded(timeline.value, itemId));
  }

  function toggleThinking(itemId: string): void {
    commit(toggleThinkingExpanded(timeline.value, itemId));
  }

  function setFocusEventSeq(seq: number | null): void {
    focusEventSeq.value = seq;
  }

  function injectEventForTest(event: TypedDesktopEvent): void {
    handleDesktopEvent(event);
  }

  function flushForTest(): void {
    if (pendingDeltas.length > 0) flushDeltas();
  }

  return {
    loadState,
    errorMessage,
    timeline,
    items,
    status,
    title,
    taskId,
    sessionId,
    cursor,
    needsRefresh,
    attempt,
    draft,
    sendError,
    sendPending,
    cancelPending,
    bridgeOnline,
    focusEventSeq,
    foldExplores,
    composerCapabilities,
    isRunning,
    attach,
    detach,
    openFromSnapshot,
    openTask,
    setDraft,
    sendMessage,
    cancelTurn,
    toggleTool,
    toggleThinking,
    setFocusEventSeq,
    injectEventForTest,
    flushForTest,
    pendingUserId: () => pendingUserId,
  };
});
