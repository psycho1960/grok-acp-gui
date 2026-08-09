// GAG-008: Pinia store — snapshot/delta merge, composer, cancel, drafts.

import { computed, ref, shallowRef } from "vue";
import { defineStore } from "pinia";
import type {
  ArtifactBlobInput,
  DesktopBridge,
  ArtifactDescriptor,
  ModeInfo,
  ModelInfo,
  ReasoningEffort,
  SlashCommandInfo,
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
  isWorkspaceStrategy,
  type WorkspaceStrategy,
} from "./mode-workspace";
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

const REASONING_LEVELS: ReasoningEffort[] = ["low", "medium", "high", "max"];

function normalizeReasoning(value: unknown): ReasoningEffort | null {
  return typeof value === "string" &&
    (REASONING_LEVELS as string[]).includes(value)
    ? (value as ReasoningEffort)
    : null;
}

export const useConversationStore = defineStore("conversation", () => {
  const loadState = ref<ConversationLoadState>("idle");
  const errorMessage = ref<string | null>(null);
  const timeline = shallowRef<ConversationState>(createEmptyConversationState());
  const draft = ref("");
  const sendError = ref<string | null>(null);
  const sendPending = ref(false);
  const attachmentPending = ref(false);
  const attachments = ref<ComposerAttachment[]>([]);
  const artifactRevision = ref(0);
  const cancelPending = ref(false);
  const bridgeOnline = ref(true);
  const focusEventSeq = ref<number | null>(null);
  /** When true, explore read tools are folded. */
  const foldExplores = ref(true);
  /** Runtime model capabilities (from bootstrap capability snapshot). */
  const models = ref<ModelInfo[]>([]);
  /** Runtime session modes (from bootstrap capability snapshot). */
  const modes = ref<ModeInfo[]>([]);
  /** Slash commands discovered from ACP `available_commands`. */
  const slashCommands = ref<SlashCommandInfo[]>([]);
  /** Per-task session mode selection persisted via session.configure. */
  const selectedMode = ref<string | null>(null);
  /** Per-task workspace strategy persisted via session.configure. */
  const workspaceStrategy = ref<WorkspaceStrategy | null>(null);
  /** Per-task model selection persisted via session.configure. */
  const selectedModel = ref<string | null>(null);
  /** Per-task reasoning effort selection persisted via session.configure. */
  const selectedReasoning = ref<ReasoningEffort | null>(null);

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
    if (event.type === "artifact.available") artifactRevision.value += 1;
    if (event.type === "session.commands.updated") {
      const commands = event.payload.commands;
      if (Array.isArray(commands)) slashCommands.value = commands;
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
      // Load runtime capabilities (models, modes, slash commands). Failures
      // only degrade to empty lists; the conversation itself stays usable.
      try {
        const snapshot = await facade.bootstrap();
        models.value = Array.isArray(snapshot.capabilities?.models)
          ? snapshot.capabilities.models
          : [];
        modes.value = Array.isArray(snapshot.capabilities?.modes)
          ? snapshot.capabilities.modes
          : [];
        slashCommands.value = Array.isArray(snapshot.capabilities?.slashCommands)
          ? snapshot.capabilities.slashCommands
          : [];
        if (snapshot.capabilities?.modelState?.currentModelId) {
          selectedModel.value = snapshot.capabilities.modelState.currentModelId;
        }
        if (snapshot.capabilities?.modeState?.currentModeId) {
          selectedMode.value = snapshot.capabilities.modeState.currentModeId;
        }
      } catch {
        // non-fatal
      }
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
    selectedMode.value = null;
    workspaceStrategy.value = null;
    selectedModel.value = null;
    selectedReasoning.value = null;
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
    if (snapshot.mode !== undefined) selectedMode.value = snapshot.mode ?? null;
    if (snapshot.workspaceStrategy !== undefined) {
      workspaceStrategy.value = isWorkspaceStrategy(snapshot.workspaceStrategy)
        ? snapshot.workspaceStrategy
        : null;
    }
    if (snapshot.model !== undefined) selectedModel.value = snapshot.model ?? null;
    if (snapshot.reasoning !== undefined) {
      selectedReasoning.value = normalizeReasoning(snapshot.reasoning);
    }
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
        if (data.mode !== undefined) selectedMode.value = data.mode ?? null;
        if (data.workspaceStrategy !== undefined) {
          workspaceStrategy.value = isWorkspaceStrategy(data.workspaceStrategy)
            ? data.workspaceStrategy
            : null;
        }
        if (data.model !== undefined) selectedModel.value = data.model ?? null;
        if (data.reasoning !== undefined) {
          selectedReasoning.value = normalizeReasoning(data.reasoning);
        }
        commitSnapshot({
          taskId: data.taskId,
          sessionId: data.sessionId,
          title: data.title || title,
          status,
          cursor,
          events: data.events,
          attempt: data.attempt,
          mode: data.mode,
          workspaceStrategy: data.workspaceStrategy,
          model: data.model,
          reasoning: data.reasoning,
        });
      } else if (data?.title) {
        if (data.mode !== undefined) selectedMode.value = data.mode ?? null;
        if (data.workspaceStrategy !== undefined) {
          workspaceStrategy.value = isWorkspaceStrategy(data.workspaceStrategy)
            ? data.workspaceStrategy
            : null;
        }
        if (data.model !== undefined) selectedModel.value = data.model ?? null;
        if (data.reasoning !== undefined) {
          selectedReasoning.value = normalizeReasoning(data.reasoning);
        }
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
    if (!paths.length) return false;
    if (!timeline.value.taskId || !facade) {
      sendError.value = "请先打开任务，再添加图片";
      return false;
    }
    attachmentPending.value = true;
    sendError.value = null;
    try {
      const result = await facade.importArtifacts(timeline.value.taskId, paths);
      if (result.success === "false") throw new Error(result.error.message);
      const imported = result.data?.artifacts ?? [];
      mergeImported(imported);
      return imported.length > 0;
    } catch (error) {
      sendError.value = error instanceof Error ? error.message : "图片导入失败";
      return false;
    } finally {
      attachmentPending.value = false;
    }
  }

  function removeAttachment(artifactId: string): void {
    if (!sendPending.value) attachments.value = attachments.value.filter((item) => item.artifactId !== artifactId);
  }

  /** Merge imported descriptors into the pending attachment list. */
  function mergeImported(imported: ArtifactDescriptor[]): void {
    const indexed = new Map(attachments.value.map((item) => [item.artifactId, item]));
    for (const item of imported) indexed.set(item.artifactId, item);
    attachments.value = Array.from(indexed.values());
    artifactRevision.value += 1;
  }

  /**
   * Import clipboard image blobs (screenshots pasted without a filesystem
   * path) through the same managed-artifact pipeline as file imports.
   */
  async function importAttachmentBlobs(blobs: ArtifactBlobInput[]): Promise<boolean> {
    if (!blobs.length) return false;
    if (!timeline.value.taskId || !facade) {
      sendError.value = "请先打开任务，再粘贴图片";
      return false;
    }
    attachmentPending.value = true;
    sendError.value = null;
    try {
      const result = await facade.importArtifactBlobs(timeline.value.taskId, blobs);
      if (result.success === "false") throw new Error(result.error.message);
      const imported = result.data?.artifacts ?? [];
      if (imported.length === 0) {
        sendError.value = "图片导入失败：未返回任何附件";
        return false;
      }
      mergeImported(imported);
      return true;
    } catch (error) {
      sendError.value = error instanceof Error ? error.message : "剪贴板图片导入失败";
      return false;
    } finally {
      attachmentPending.value = false;
    }
  }

  /**
   * Persist the session mode for the current task (agent/plan/ask). Every
   * following turn then sends session/set_mode with the new modeId.
   */
  async function configureMode(mode: string | null): Promise<boolean> {
    const previous = selectedMode.value;
    selectedMode.value = mode;
    if (!timeline.value.taskId || !facade) return false;
    try {
      const result = await facade.configureSession(timeline.value.taskId, {
        mode,
      });
      if (result.success === "false") throw new Error(result.error.message);
      return true;
    } catch (error) {
      selectedMode.value = previous;
      sendError.value = error instanceof Error ? error.message : "模式切换失败";
      return false;
    }
  }

  /**
   * Persist the workspace strategy for the current task (worktree/readonly/
   * direct). The next session start resolves its cwd from this value.
   */
  async function configureWorkspaceStrategy(
    strategy: WorkspaceStrategy,
  ): Promise<boolean> {
    const previous = workspaceStrategy.value;
    workspaceStrategy.value = strategy;
    if (!timeline.value.taskId || !facade) return false;
    try {
      const result = await facade.configureSession(timeline.value.taskId, {
        workspaceStrategy: strategy,
      });
      if (result.success === "false") throw new Error(result.error.message);
      return true;
    } catch (error) {
      workspaceStrategy.value = previous;
      sendError.value = error instanceof Error ? error.message : "工作区策略切换失败";
      return false;
    }
  }

  /**
   * Persist the model selection for the current task. Every following turn
   * then carries the new model in its ACP prompt request.
   */
  async function configureModel(model: string | null): Promise<boolean> {
    const previous = selectedModel.value;
    selectedModel.value = model;
    if (!timeline.value.taskId || !facade) return false;
    try {
      const result = await facade.configureSession(timeline.value.taskId, {
        model,
      });
      if (result.success === "false") throw new Error(result.error.message);
      return true;
    } catch (error) {
      selectedModel.value = previous;
      sendError.value = error instanceof Error ? error.message : "模型切换失败";
      return false;
    }
  }

  /** Persist the reasoning effort selection for the current task. */
  async function configureReasoning(reasoning: ReasoningEffort): Promise<boolean> {
    const previous = selectedReasoning.value;
    selectedReasoning.value = reasoning;
    if (!timeline.value.taskId || !facade) return false;
    try {
      const result = await facade.configureSession(timeline.value.taskId, {
        reasoning,
      });
      if (result.success === "false") throw new Error(result.error.message);
      return true;
    } catch (error) {
      selectedReasoning.value = previous;
      sendError.value = error instanceof Error ? error.message : "推理强度切换失败";
      return false;
    }
  }

  async function sendMessage(): Promise<boolean> {
    const text = draft.value.trim();
    const outgoingAttachments = attachments.value.map((attachment) => ({ ...attachment }));
    const attachmentIds = outgoingAttachments.map((attachment) => attachment.artifactId);
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
      appendUserMessage(timeline.value, redactVisibleText(text || `已添加 ${attachmentIds.length} 张图片附件`), {
        id: localId,
        pending: true,
        attachments: outgoingAttachments,
      }),
    );
    draft.value = "";
    attachments.value = [];
    clearDraft(timeline.value.taskId);
    // Enter running before IPC. ACP events may complete the turn before the
    // execute Promise resolves; setting it afterwards would overwrite idle.
    commit(setRunStatus(timeline.value, "running"));

    try {
      const result = await facade.sendTurn(timeline.value.taskId, text, attachmentIds);
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
        if (!attachments.value.length) attachments.value = outgoingAttachments;
        return false;
      }
      commit(markUserMessageConfirmed(timeline.value, localId));
      draft.value = "";
      clearDraft(timeline.value.taskId);
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
      if (!attachments.value.length) attachments.value = outgoingAttachments;
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
    artifactRevision,
    cancelPending,
    bridgeOnline,
    focusEventSeq,
    foldExplores,
    models,
    modes,
    slashCommands,
    selectedMode,
    workspaceStrategy,
    selectedModel,
    selectedReasoning,
    composerCapabilities,
    isRunning,
    attach,
    detach,
    openFromSnapshot,
    openTask,
    setDraft,
    importAttachmentPaths,
    importAttachmentBlobs,
    configureMode,
    configureWorkspaceStrategy,
    configureModel,
    configureReasoning,
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
