// GAG-008: Pinia store — snapshot/delta merge, composer, cancel, drafts.

import { computed, ref, shallowRef } from "vue";
import { defineStore } from "pinia";
import type {
  DesktopBridge,
  ArtifactDescriptor,
  TaskOpenResult,
  TaskId,
  TypedDesktopEvent,
} from "../../bridge/types";
import {
  createConversationFacade,
  type ConversationFacade,
} from "./conversation-facade";
import { clearDraft, loadDraft, saveDraft } from "./draft";
import { redactVisibleText } from "./markdown";
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
  updateApprovalDecision,
} from "./reducer";
import type {
  ComposerCapabilities,
  ComposerAttachment,
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
  const attachmentPending = ref(false);
  const attachments = ref<ComposerAttachment[]>([]);
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
  let openVersion = 0;

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
    openVersion += 1;
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

  function commitSnapshot(snapshot: SessionTimelineSnapshot): void {
    loadState.value = "loading";
    const next = applySnapshot(createEmptyConversationState(snapshot.taskId), snapshot);
    commit(next);
    draft.value = loadDraft(snapshot.taskId);
    attachments.value = [];
    sendError.value = null;
    loadState.value = "ready";
    errorMessage.value = null;
  }

  function openFromSnapshot(snapshot: SessionTimelineSnapshot): void {
    openVersion += 1;
    commitSnapshot(snapshot);
  }

  async function openTask(taskId: TaskId, title = "任务会话"): Promise<void> {
    const version = ++openVersion;
    loadState.value = "loading";
    errorMessage.value = null;
    commit({
      ...createEmptyConversationState(taskId),
      title,
      status: "idle",
    });
    draft.value = loadDraft(taskId);
    attachments.value = [];

    if (!facade) {
      loadState.value = "ready";
      return;
    }

    try {
      const result = await facade.openTask(taskId);
      if (version !== openVersion || disposed) return;
      if (result.success === "false") {
        errorMessage.value = result.error.message;
        loadState.value = "error";
        return;
      }
      const data = result.data as TaskOpenResult | undefined;
      if (data?.sessionId && Array.isArray(data.events)) {
        const cursor =
          typeof data.cursor === "number"
            ? data.cursor
            : (data.cursor?.lastSeq ?? data.cursor?.snapshotSeq ?? 0);
        const status: ConversationState["status"] =
          data.status === "running" || data.status === "preparing"
            ? "running"
            : data.status === "waiting_permission"
              ? "waiting_permission"
              : data.status === "failed" ||
                  data.status === "interrupted" ||
                  data.status === "conflicted"
                ? "error"
                : "idle";
        commitSnapshot({
          taskId: data.taskId,
          sessionId: data.sessionId,
          title: data.title || title,
          status,
          cursor,
          events: data.events,
          attempt: data.attempt,
        });
      } else if (data?.title) {
        commit({
          ...timeline.value,
          title: data.title,
          status:
            data.status === "running" || data.status === "preparing"
              ? "running"
              : data.status === "waiting_permission"
                ? "waiting_permission"
                : data.status === "failed" || data.status === "interrupted"
                  ? "error"
                  : "idle",
        });
      }
      loadState.value = "ready";
    } catch (error) {
      if (version !== openVersion || disposed) return;
      errorMessage.value =
        error instanceof Error ? error.message : "打开任务失败";
      loadState.value = "error";
    }
  }

  function setDraft(text: string): void {
    draft.value = text;
    saveDraft(timeline.value.taskId, text);
  }

  async function importAttachmentPaths(paths: string[]): Promise<boolean> {
    if (paths.length === 0) return false;
    if (!timeline.value.taskId || !facade) {
      sendError.value = "请先打开任务，再添加图片";
      return false;
    }
    attachmentPending.value = true;
    sendError.value = null;
    try {
      const result = await facade.importArtifacts(timeline.value.taskId, paths);
      if (result.success === "false") {
        sendError.value = result.error.message;
        return false;
      }
      const imported = Array.isArray(result.data?.artifacts)
        ? result.data.artifacts
        : [];
      const byId = new Map(attachments.value.map((item) => [item.artifactId, item]));
      for (const item of imported as ArtifactDescriptor[]) {
        byId.set(item.artifactId, item);
      }
      attachments.value = Array.from(byId.values());
      return imported.length > 0;
    } catch (error) {
      sendError.value = error instanceof Error ? error.message : "图片导入失败";
      return false;
    } finally {
      attachmentPending.value = false;
    }
  }

  function removeAttachment(artifactId: string): void {
    if (sendPending.value) return;
    attachments.value = attachments.value.filter((item) => item.artifactId !== artifactId);
  }

  async function sendMessage(): Promise<boolean> {
    const text = draft.value.trim();
    const outgoingAttachments = attachments.value.map((item) => ({ ...item }));
    const attachmentIds = outgoingAttachments.map((item) => item.artifactId);
    if (!text && attachmentIds.length === 0) return false;
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
      appendUserMessage(
        timeline.value,
        redactVisibleText(text || `已添加 ${attachmentIds.length} 张图片附件`),
        {
          id: localId,
          pending: true,
          attachments: outgoingAttachments,
        },
      ),
    );
    // Ownership moves to the optimistic timeline item immediately. The vision
    // preprocessing request may take time, but sent content must not remain in
    // the Composer while it is pending.
    draft.value = "";
    attachments.value = [];
    clearDraft(timeline.value.taskId);
    // Enter running before IPC. ACP events may complete the turn before the
    // execute Promise resolves; setting it afterwards would overwrite idle.
    commit(setRunStatus(timeline.value, "running"));

    try {
      const result = await facade.sendTurn(
        timeline.value.taskId,
        text,
        attachmentIds,
      );
      if (result.success === "false") {
        commit(
          markUserMessageFailed(
            timeline.value,
            localId,
            result.error.message,
          ),
        );
        sendError.value = result.error.message;
        commit(setRunStatus(timeline.value, "error"));
        if (!draft.value) {
          draft.value = text;
          saveDraft(timeline.value.taskId, text);
        }
        if (attachments.value.length === 0) attachments.value = outgoingAttachments;
        return false;
      }
      commit(markUserMessageConfirmed(timeline.value, localId));
      pendingUserId = null;
      return true;
    } catch (error) {
      const msg = error instanceof Error ? error.message : "发送失败";
      commit(markUserMessageFailed(timeline.value, localId, msg));
      commit(setRunStatus(timeline.value, "error"));
      sendError.value = msg;
      if (!draft.value) {
        draft.value = text;
        saveDraft(timeline.value.taskId, text);
      }
      if (attachments.value.length === 0) attachments.value = outgoingAttachments;
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

  async function resolvePermission(itemId: string, optionId: string): Promise<boolean> {
    if (!facade) return false;
    const item = timeline.value.items.find((candidate) => candidate.id === itemId);
    if (!item || item.kind !== "permission" || item.slot.decisionState === "submitting") {
      return false;
    }
    const slot = item.slot;
    if (slot.expired || Date.now() / 1000 > slot.expiresAtEpochSeconds) {
      commit(updateApprovalDecision(timeline.value, itemId, {
        decisionState: "error",
        errorMessage: "请求已失效",
      }));
      return false;
    }
    commit(updateApprovalDecision(timeline.value, itemId, {
      decisionState: "submitting",
      optionId,
    }));
    try {
      const result = await facade.resolvePermission({
        taskId: slot.taskId,
        sessionId: slot.sessionId,
        requestId: slot.requestId,
        correlationId: slot.correlationId,
        expectedVersion: slot.expectedVersion,
        optionId,
      });
      if (result.success === "false") throw new Error(result.error.message);
      commit(updateApprovalDecision(timeline.value, itemId, {
        decisionState: "resolved",
        optionId,
      }));
      return true;
    } catch (error) {
      commit(updateApprovalDecision(timeline.value, itemId, {
        decisionState: "error",
        optionId,
        errorMessage: error instanceof Error ? error.message : "权限处理失败",
      }));
      return false;
    }
  }

  async function resolvePlan(itemId: string, optionId: string): Promise<boolean> {
    if (!facade) return false;
    const item = timeline.value.items.find((candidate) => candidate.id === itemId);
    if (!item || item.kind !== "plan" || item.slot.decisionState === "submitting") {
      return false;
    }
    const slot = item.slot;
    if (slot.approvalInvalidated) return false;
    commit(updateApprovalDecision(timeline.value, itemId, {
      decisionState: "submitting",
      optionId,
    }));
    try {
      const result = await facade.resolvePlan({
        taskId: slot.taskId,
        sessionId: slot.sessionId,
        requestId: slot.requestId,
        correlationId: slot.correlationId,
        expectedVersion: slot.version,
        optionId,
      });
      if (result.success === "false") throw new Error(result.error.message);
      const state =
        result.data && typeof result.data === "object" && "state" in result.data
          ? String(result.data.state)
          : "resolved";
      commit(updateApprovalDecision(timeline.value, itemId, {
        decisionState: "resolved",
        optionId,
        status: state,
      }));
      return true;
    } catch (error) {
      commit(updateApprovalDecision(timeline.value, itemId, {
        decisionState: "error",
        optionId,
        errorMessage: error instanceof Error ? error.message : "Plan 处理失败",
      }));
      return false;
    }
  }

  async function resumeSession(): Promise<boolean> {
    if (!timeline.value.taskId || !facade) return false;
    sendError.value = null;
    try {
      const result = await facade.resumeSession(timeline.value.taskId);
      if (result.success === "false") {
        sendError.value = result.error.message;
        return false;
      }
      await openTask(timeline.value.taskId);
      return true;
    } catch (error) {
      sendError.value = error instanceof Error ? error.message : "恢复会话失败";
      return false;
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
    attachmentPending,
    attachments,
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
    importAttachmentPaths,
    removeAttachment,
    sendMessage,
    cancelTurn,
    resumeSession,
    resolvePermission,
    resolvePlan,
    toggleTool,
    toggleThinking,
    setFocusEventSeq,
    injectEventForTest,
    flushForTest,
    pendingUserId: () => pendingUserId,
  };
});
