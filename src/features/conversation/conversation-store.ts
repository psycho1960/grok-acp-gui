// GAG-008: Pinia store — snapshot/delta merge, composer, cancel, drafts.

import { computed, ref, shallowRef, watch } from "vue";
import { acceptHMRUpdate, defineStore } from "pinia";
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
  WorktreeRecord,
} from "../../bridge/types";
import {
  createConversationFacade,
  type ConversationFacade,
} from "./conversation-facade";
import { clearDraft, loadDraft, saveDraft } from "./draft";
import { redactVisibleText } from "./markdown";
import {
  isWorkspaceStrategy,
  WORKTREE_NOT_READY_MESSAGE,
  workspaceStrategyForMode,
  type WorkspaceStrategy,
} from "./mode-workspace";
import {
  applyEvent,
  applySnapshot,
  appendUserMessage,
  createEmptyConversationState,
  foldExploreTools,
  isOffTimelineStatus,
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
  QueuedFollowUp,
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

function desktopErrorMessage(error: { code: string; message: string }): string {
  return error.code === "WORKTREE_NOT_READY"
    ? WORKTREE_NOT_READY_MESSAGE
    : error.message;
}

export const useConversationStore = defineStore("conversation", () => {
  const loadState = ref<ConversationLoadState>("idle");
  const errorMessage = ref<string | null>(null);
  const timeline = shallowRef<ConversationState>(
    createEmptyConversationState(),
  );
  const draft = ref("");
  const sendError = ref<string | null>(null);
  const sendPending = ref(false);
  const attachmentPending = ref(false);
  const attachments = ref<ComposerAttachment[]>([]);
  const queuedFollowUps = ref<QueuedFollowUp[]>([]);
  let queueDrainPending = false;
  let interruptFollowUp: QueuedFollowUp | null = null;
  const artifactRevision = ref(0);
  const listedArtifacts = ref<ArtifactDescriptor[]>([]);
  const worktreeRecord = ref<WorktreeRecord | null>(null);
  const lifecycleStatus = ref<string | null>(null);
  const cancelPending = ref(false);
  const bridgeOnline = ref(true);
  const focusEventSeq = ref<number | null>(null);
  /** When true, explore read tools are folded. */
  const foldExplores = ref(true);
  /** Runtime model capabilities (from bootstrap capability snapshot). */
  const models = ref<ModelInfo[]>([]);
  /** Runtime session modes (from bootstrap capability snapshot). */
  const modes = ref<ModeInfo[]>([]);
  /** Runtime capability baseline used before the current session advertises modes. */
  const bootstrapModes = ref<ModeInfo[]>([]);
  /** Slash commands discovered from ACP `available_commands`. */
  const slashCommands = ref<SlashCommandInfo[]>([]);
  /** Runtime capability baseline used when a session has not published an override. */
  const bootstrapSlashCommands = ref<SlashCommandInfo[]>([]);
  /** Per-task session mode selection persisted via session.configure. */
  const selectedMode = ref<string | null>(null);
  /** Per-task workspace strategy persisted via session.configure. */
  const workspaceStrategy = ref<WorkspaceStrategy | null>(null);
  /** Backend-verified availability; null means the snapshot did not say. */
  const workspaceAvailable = ref<boolean | null>(null);
  /** Per-task model selection persisted via session.configure. */
  const selectedModel = ref<string | null>(null);
  /** Per-task reasoning effort selection persisted via session.configure. */
  const selectedReasoning = ref<ReasoningEffort | null>(null);
  /** Prevent overlapping settings writes and expose a truthful pending state. */
  const settingsPending = ref(false);

  let facade: ConversationFacade | null = null;
  let unsubscribe: (() => void) | null = null;
  let disposed = false;
  let pendingDeltas: TypedDesktopEvent[] = [];
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingUserId: string | null = null;
  let openVersion = 0;
  let settingsVersion = 0;

  const items = computed<TimelineItem[]>(() => {
    const raw = timeline.value.items;
    const folded = foldExplores.value ? foldExploreTools(raw) : raw;
    return folded.filter((item) => !isOffTimelineStatus(item));
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

  const workspaceNotice = computed(() => {
    if (
      workspaceStrategy.value === "worktree" &&
      workspaceAvailable.value === false
    ) {
      return WORKTREE_NOT_READY_MESSAGE;
    }
    if (workspaceStrategy.value === "readonly") {
      return "只读策略已启用：使用项目目录，但写入与非只读操作会被后端拒绝。";
    }
    return null;
  });

  const hasArtifacts = computed(
    () =>
      listedArtifacts.value.length > 0 ||
      items.value.some((item) => item.kind === "artifact"),
  );

  const workspaceAttention = computed(() => {
    if (lifecycleStatus.value === "conflicted") return "conflicted";
    if (
      workspaceStrategy.value === "worktree" &&
      workspaceAvailable.value === false
    ) {
      return "not-created";
    }
    const state = worktreeRecord.value?.state;
    if (
      state === "missing" ||
      state === "creation_failed" ||
      state === "allocating"
    ) {
      return "not-created";
    }
    if (worktreeRecord.value?.ownership === "external") {
      return "external-awaiting-adoption";
    }
    if (state === "orphaned" || state === "quarantined") {
      return "cleanup-recovery-pending";
    }
    return null;
  });

  const railNeeded = computed(
    () => hasArtifacts.value || workspaceAttention.value != null,
  );

  function clearRailContext(): void {
    listedArtifacts.value = [];
    worktreeRecord.value = null;
    lifecycleStatus.value = null;
  }

  async function refreshRailContext(): Promise<void> {
    const current = facade;
    const id = taskId.value;
    if (!current || !id) {
      listedArtifacts.value = [];
      worktreeRecord.value = null;
      return;
    }
    try {
      const listed = await current.listArtifacts(id);
      listedArtifacts.value =
        listed.success === "true" ? (listed.data?.artifacts ?? []) : [];
    } catch {
      listedArtifacts.value = [];
    }
    if (
      workspaceStrategy.value !== "worktree" ||
      workspaceAvailable.value === false
    ) {
      worktreeRecord.value = null;
      return;
    }
    try {
      const inspected = await current.inspectWorktree(id);
      worktreeRecord.value =
        inspected.success === "true"
          ? (inspected.data?.worktree ?? null)
          : null;
    } catch {
      worktreeRecord.value = null;
    }
  }

  const composerCapabilities = computed<ComposerCapabilities>(() => {
    if (!bridgeOnline.value) {
      return {
        canSend: false,
        canCancel: false,
        disabledReason: "Bridge 离线，草稿已保留",
        bridgeOnline: false,
      };
    }
    if (settingsPending.value) {
      return {
        canSend: false,
        canCancel: false,
        disabledReason: "正在保存会话设置…",
        bridgeOnline: true,
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
    if (st === "waiting_permission" || st === "waiting_plan") {
      return {
        canSend: false,
        canCancel: true,
        disabledReason: st === "waiting_plan" ? "等待计划审批" : "等待权限审批",
        bridgeOnline: true,
      };
    }
    // idle | error — allow compose
    return {
      canSend: true,
      canCancel: false,
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
    if (event.type === "artifact.available") {
      artifactRevision.value += 1;
      void refreshRailContext();
    }
    if (
      event.type === "session.capabilities.updated" &&
      (!timeline.value.taskId || event.taskId === timeline.value.taskId) &&
      (!timeline.value.sessionId ||
        event.sessionId === timeline.value.sessionId)
    ) {
      if (Array.isArray(event.payload.modes)) modes.value = event.payload.modes;
      if (
        Array.isArray(event.payload.models) &&
        event.payload.models.length > 0
      ) {
        models.value = event.payload.models;
      }
    }
    if (event.type === "session.commands.updated") {
      const commands = event.payload.commands;
      if (Array.isArray(commands)) slashCommands.value = commands;
    }
    scheduleDelta(event);
  }

  async function attach(bridge: DesktopBridge): Promise<void> {
    disposed = false;
    bootstrapSlashCommands.value = [];
    bootstrapModes.value = [];
    slashCommands.value = [];
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
        bootstrapModes.value = Array.isArray(snapshot.capabilities?.modes)
          ? snapshot.capabilities.modes
          : [];
        modes.value = bootstrapModes.value;
        bootstrapSlashCommands.value = Array.isArray(
          snapshot.capabilities?.slashCommands,
        )
          ? snapshot.capabilities.slashCommands
          : [];
        slashCommands.value = bootstrapSlashCommands.value;
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
      errorMessage.value = error instanceof Error ? error.message : "订阅失败";
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
    workspaceAvailable.value = null;
    selectedModel.value = null;
    selectedReasoning.value = null;
    settingsPending.value = false;
    clearRailContext();
    queuedFollowUps.value = [];
    interruptFollowUp = null;
    queueDrainPending = false;
  }

  function commitSnapshot(
    snapshot: SessionTimelineSnapshot,
    preserveComposer = false,
  ): void {
    loadState.value = "loading";
    const next = applySnapshot(
      createEmptyConversationState(snapshot.taskId),
      snapshot,
    );
    commit(next);
    const latestCapabilities = [...snapshot.events]
      .reverse()
      .find(
        (
          event,
        ): event is Extract<
          TypedDesktopEvent,
          { type: "session.capabilities.updated" }
        > =>
          event.type === "session.capabilities.updated" &&
          event.sessionId === snapshot.sessionId,
      );
    modes.value = Array.isArray(latestCapabilities?.payload.modes)
      ? latestCapabilities.payload.modes
      : bootstrapModes.value;
    if (
      Array.isArray(latestCapabilities?.payload.models) &&
      latestCapabilities.payload.models.length > 0
    ) {
      models.value = latestCapabilities.payload.models;
    }
    const latestCommands = [...snapshot.events]
      .reverse()
      .find(
        (
          event,
        ): event is Extract<
          TypedDesktopEvent,
          { type: "session.commands.updated" }
        > =>
          event.type === "session.commands.updated" &&
          event.sessionId === snapshot.sessionId,
      );
    slashCommands.value = Array.isArray(latestCommands?.payload.commands)
      ? latestCommands.payload.commands
      : bootstrapSlashCommands.value;
    if (!preserveComposer) {
      draft.value = loadDraft(snapshot.taskId);
      attachments.value = [];
      queuedFollowUps.value = [];
      interruptFollowUp = null;
      queueDrainPending = false;
    }
    sendError.value = null;
    loadState.value = "ready";
    errorMessage.value = null;
    if (snapshot.mode !== undefined) selectedMode.value = snapshot.mode ?? null;
    if (snapshot.workspaceStrategy !== undefined) {
      workspaceStrategy.value = isWorkspaceStrategy(snapshot.workspaceStrategy)
        ? snapshot.workspaceStrategy
        : null;
    }
    if (snapshot.workspaceAvailable !== undefined) {
      workspaceAvailable.value = snapshot.workspaceAvailable === true;
    }
    if (snapshot.model !== undefined)
      selectedModel.value = snapshot.model ?? null;
    if (snapshot.reasoning !== undefined) {
      selectedReasoning.value = normalizeReasoning(snapshot.reasoning);
    }
    lifecycleStatus.value = snapshot.taskStatus ?? null;
  }

  function openFromSnapshot(snapshot: SessionTimelineSnapshot): void {
    openVersion += 1;
    settingsVersion += 1;
    settingsPending.value = false;
    commitSnapshot(snapshot);
  }

  async function openTask(
    taskId: TaskId,
    title = "新任务",
    preserveComposer = false,
  ): Promise<void> {
    const version = ++openVersion;
    settingsVersion += 1;
    settingsPending.value = false;
    loadState.value = "loading";
    errorMessage.value = null;
    commit({
      ...createEmptyConversationState(taskId),
      title,
      status: "idle",
    });
    if (!preserveComposer) {
      draft.value = loadDraft(taskId);
      attachments.value = [];
      queuedFollowUps.value = [];
      interruptFollowUp = null;
      queueDrainPending = false;
    }
    selectedMode.value = null;
    workspaceStrategy.value = null;
    workspaceAvailable.value = null;
    lifecycleStatus.value = null;
    selectedModel.value = null;
    selectedReasoning.value = null;
    modes.value = bootstrapModes.value;

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
        lifecycleStatus.value = data.status ?? null;
        commitSnapshot(
          {
            taskId: data.taskId,
            sessionId: data.sessionId,
            title: data.title || title,
            status,
            cursor,
            events: data.events,
            attempt: data.attempt,
            mode: data.mode,
            workspaceStrategy: data.workspaceStrategy,
            workspaceAvailable: data.workspaceAvailable,
            model: data.model,
            reasoning: data.reasoning,
            taskStatus: data.status,
          },
          preserveComposer,
        );
      } else if (data) {
        if (data.mode !== undefined) selectedMode.value = data.mode ?? null;
        if (data.workspaceStrategy !== undefined) {
          workspaceStrategy.value = isWorkspaceStrategy(data.workspaceStrategy)
            ? data.workspaceStrategy
            : null;
        }
        if (data.workspaceAvailable !== undefined) {
          workspaceAvailable.value = data.workspaceAvailable === true;
        }
        if (data.model !== undefined) selectedModel.value = data.model ?? null;
        if (data.reasoning !== undefined) {
          selectedReasoning.value = normalizeReasoning(data.reasoning);
        }
        lifecycleStatus.value = data.status ?? null;
        commit({
          ...timeline.value,
          title: data.title?.trim() || title,
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
      await refreshRailContext();
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
    if (!sendPending.value)
      attachments.value = attachments.value.filter(
        (item) => item.artifactId !== artifactId,
      );
  }

  /** Merge imported descriptors into the pending attachment list. */
  function mergeImported(imported: ArtifactDescriptor[]): void {
    const indexed = new Map(
      attachments.value.map((item) => [item.artifactId, item]),
    );
    for (const item of imported) indexed.set(item.artifactId, item);
    attachments.value = Array.from(indexed.values());
    artifactRevision.value += 1;
  }

  /**
   * Import clipboard image blobs (screenshots pasted without a filesystem
   * path) through the same managed-artifact pipeline as file imports.
   */
  async function importAttachmentBlobs(
    blobs: ArtifactBlobInput[],
  ): Promise<boolean> {
    if (!blobs.length) return false;
    if (!timeline.value.taskId || !facade) {
      sendError.value = "请先打开任务，再粘贴图片";
      return false;
    }
    attachmentPending.value = true;
    sendError.value = null;
    try {
      const result = await facade.importArtifactBlobs(
        timeline.value.taskId,
        blobs,
      );
      if (result.success === "false") throw new Error(result.error.message);
      const imported = result.data?.artifacts ?? [];
      if (imported.length === 0) {
        sendError.value = "图片导入失败：未返回任何附件";
        return false;
      }
      mergeImported(imported);
      return true;
    } catch (error) {
      sendError.value =
        error instanceof Error ? error.message : "剪贴板图片导入失败";
      return false;
    } finally {
      attachmentPending.value = false;
    }
  }

  type StableSettings = {
    mode?: string | null;
    workspaceStrategy?: WorkspaceStrategy;
    model?: string | null;
    reasoning?: ReasoningEffort;
  };

  function applyStableSettings(
    source: Record<string, unknown>,
    fallback: StableSettings = {},
  ): void {
    const mode = source.mode !== undefined ? source.mode : fallback.mode;
    if (mode !== undefined)
      selectedMode.value = typeof mode === "string" ? mode : null;

    const strategy = source.workspaceStrategy ?? fallback.workspaceStrategy;
    if (strategy !== undefined && isWorkspaceStrategy(strategy)) {
      workspaceStrategy.value = strategy;
    }
    if (source.workspaceAvailable !== undefined) {
      workspaceAvailable.value = source.workspaceAvailable === true;
    } else if (fallback.workspaceStrategy === "worktree") {
      // Never invent a ready state when the backend omitted availability.
      workspaceAvailable.value = false;
    } else if (fallback.workspaceStrategy) {
      workspaceAvailable.value = true;
    }

    const model = source.model !== undefined ? source.model : fallback.model;
    if (model !== undefined)
      selectedModel.value = typeof model === "string" ? model : null;

    const reasoning = source.reasoning ?? fallback.reasoning;
    if (reasoning !== undefined)
      selectedReasoning.value = normalizeReasoning(reasoning);
  }

  async function reloadStableSettings(taskId: TaskId): Promise<void> {
    if (!facade) return;
    try {
      const result = await facade.openTask(taskId);
      if (
        result.success === "true" &&
        timeline.value.taskId === taskId &&
        result.data &&
        typeof result.data === "object"
      ) {
        applyStableSettings(result.data as Record<string, unknown>);
      }
    } catch {
      // Keep the last backend-confirmed UI state when reloading also fails.
    }
  }

  async function configureStableSettings(
    settings: StableSettings,
    fallbackError: string,
  ): Promise<boolean> {
    const taskId = timeline.value.taskId;
    const activeFacade = facade;
    if (!taskId || !activeFacade || settingsPending.value) return false;
    const version = ++settingsVersion;
    settingsPending.value = true;
    sendError.value = null;
    try {
      const result = await activeFacade.configureSession(taskId, settings);
      if (timeline.value.taskId !== taskId || version !== settingsVersion)
        return false;
      if (result.success === "false") {
        sendError.value = desktopErrorMessage(result.error);
        await reloadStableSettings(taskId);
        return false;
      }
      const data =
        result.data && typeof result.data === "object"
          ? (result.data as Record<string, unknown>)
          : {};
      applyStableSettings(data, settings);
      return true;
    } catch (error) {
      if (timeline.value.taskId === taskId && version === settingsVersion) {
        sendError.value =
          error instanceof Error ? error.message : fallbackError;
        await reloadStableSettings(taskId);
      }
      return false;
    } finally {
      if (version === settingsVersion) settingsPending.value = false;
    }
  }

  /** Persist mode and its default workspace policy in one backend transaction. */
  async function configureMode(
    mode: string | null,
    linkedStrategy: WorkspaceStrategy | null = workspaceStrategyForMode(mode),
  ): Promise<boolean> {
    return configureStableSettings(
      linkedStrategy ? { mode, workspaceStrategy: linkedStrategy } : { mode },
      "模式切换失败",
    );
  }

  /** Persist an explicit user workspace-policy override. */
  async function configureWorkspaceStrategy(
    strategy: WorkspaceStrategy,
  ): Promise<boolean> {
    return configureStableSettings(
      { workspaceStrategy: strategy },
      "工作区策略切换失败",
    );
  }

  async function configureModel(model: string | null): Promise<boolean> {
    const reasoning = model
      ? models.value.find((candidate) => candidate.modelId === model)
          ?.reasoningEffort
      : undefined;
    return configureStableSettings(
      reasoning ? { model, reasoning } : { model },
      "模型切换失败",
    );
  }

  async function configureReasoning(
    reasoning: ReasoningEffort,
  ): Promise<boolean> {
    return configureStableSettings({ reasoning }, "推理强度切换失败");
  }

  async function sendMessage(): Promise<boolean> {
    const text = draft.value.trim();
    const outgoingAttachments = attachments.value.map((attachment) => ({
      ...attachment,
    }));
    const attachmentIds = outgoingAttachments.map(
      (attachment) => attachment.artifactId,
    );
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
            desktopErrorMessage(result.error),
          ),
        );
        sendError.value = desktopErrorMessage(result.error);
        commit(setRunStatus(timeline.value, "error"));
        if (!draft.value) {
          draft.value = text;
          saveDraft(timeline.value.taskId, text);
        }
        if (!attachments.value.length) attachments.value = outgoingAttachments;
        return false;
      }
      commit(markUserMessageConfirmed(timeline.value, localId));
      const response =
        result.data && typeof result.data === "object"
          ? (result.data as Record<string, unknown>)
          : null;
      if (
        typeof response?.taskTitle === "string" &&
        response.taskTitle.trim()
      ) {
        commit({ ...timeline.value, title: response.taskTitle.trim() });
      }
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
        sendError.value = desktopErrorMessage(result.error);
        commit(setRunStatus(timeline.value, "running"));
        return false;
      }
      return true;
    } catch (error) {
      sendError.value = error instanceof Error ? error.message : "停止失败";
      commit(setRunStatus(timeline.value, "running"));
      return false;
    } finally {
      cancelPending.value = false;
    }
  }

  function enqueueFollowUp(): boolean {
    const text = draft.value.trim();
    const outgoing = attachments.value.map((attachment) => ({ ...attachment }));
    if (!text && outgoing.length === 0) return false;
    if (composerCapabilities.value.canSend) return false;
    queuedFollowUps.value = [
      ...queuedFollowUps.value,
      {
        id: `queue-${Date.now()}-${queuedFollowUps.value.length}`,
        text,
        attachments: outgoing,
      },
    ];
    draft.value = "";
    attachments.value = [];
    if (timeline.value.taskId) clearDraft(timeline.value.taskId);
    return true;
  }

  function editFollowUp(id: string): boolean {
    const item = queuedFollowUps.value.find((entry) => entry.id === id);
    if (!item) return false;
    queuedFollowUps.value = queuedFollowUps.value.filter(
      (entry) => entry.id !== id,
    );
    draft.value = item.text;
    attachments.value = item.attachments.map((attachment) => ({
      ...attachment,
    }));
    if (timeline.value.taskId) saveDraft(timeline.value.taskId, item.text);
    return true;
  }

  function deleteFollowUp(id: string): boolean {
    const next = queuedFollowUps.value.filter((entry) => entry.id !== id);
    if (next.length === queuedFollowUps.value.length) return false;
    queuedFollowUps.value = next;
    return true;
  }

  async function drainQueuedFollowUp(): Promise<void> {
    const status = timeline.value.status;
    if (status !== "idle" && status !== "error") return;
    if (
      queueDrainPending ||
      sendPending.value ||
      !composerCapabilities.value.canSend
    )
      return;
    const next = interruptFollowUp ?? queuedFollowUps.value[0];
    if (!next) return;
    queueDrainPending = true;
    const preservedDraft = draft.value;
    const preservedAttachments = attachments.value.map((attachment) => ({
      ...attachment,
    }));
    try {
      if (interruptFollowUp?.id === next.id) interruptFollowUp = null;
      else
        queuedFollowUps.value = queuedFollowUps.value.filter(
          (entry) => entry.id !== next.id,
        );
      draft.value = next.text;
      attachments.value = next.attachments.map((attachment) => ({
        ...attachment,
      }));
      await sendMessage();
      draft.value = preservedDraft;
      attachments.value = preservedAttachments;
      if (timeline.value.taskId)
        saveDraft(timeline.value.taskId, preservedDraft);
    } finally {
      queueDrainPending = false;
    }
  }

  async function sendFollowUpNow(id: string): Promise<boolean> {
    if (interruptFollowUp || cancelPending.value) return false;
    const item = queuedFollowUps.value.find((entry) => entry.id === id);
    if (!item) return false;
    queuedFollowUps.value = queuedFollowUps.value.filter(
      (entry) => entry.id !== id,
    );
    interruptFollowUp = item;
    if (composerCapabilities.value.canCancel) {
      const cancelled = await cancelTurn();
      if (!cancelled && !composerCapabilities.value.canSend) {
        interruptFollowUp = null;
        queuedFollowUps.value = [item, ...queuedFollowUps.value];
        return false;
      }
    }
    if (composerCapabilities.value.canSend) {
      await drainQueuedFollowUp();
    }
    return true;
  }

  watch(
    () => [composerCapabilities.value.canSend, timeline.value.status] as const,
    () => {
      void drainQueuedFollowUp();
    },
  );

  async function resolvePermission(
    itemId: string,
    optionId: string,
  ): Promise<boolean> {
    if (!facade) return false;
    const item = timeline.value.items.find(
      (candidate) => candidate.id === itemId,
    );
    if (
      !item ||
      item.kind !== "permission" ||
      item.slot.decisionState === "submitting"
    ) {
      return false;
    }
    const slot = item.slot;
    if (slot.expired || Date.now() / 1000 > slot.expiresAtEpochSeconds) {
      commit(
        updateApprovalDecision(timeline.value, itemId, {
          decisionState: "error",
          errorMessage: "请求已失效",
        }),
      );
      return false;
    }
    commit(
      updateApprovalDecision(timeline.value, itemId, {
        decisionState: "submitting",
        optionId,
      }),
    );
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
      commit(
        updateApprovalDecision(timeline.value, itemId, {
          decisionState: "resolved",
          optionId,
        }),
      );
      return true;
    } catch (error) {
      commit(
        updateApprovalDecision(timeline.value, itemId, {
          decisionState: "error",
          optionId,
          errorMessage: error instanceof Error ? error.message : "权限处理失败",
        }),
      );
      return false;
    }
  }

  async function resolvePlan(
    itemId: string,
    optionId: string,
  ): Promise<boolean> {
    if (!facade) return false;
    const item = timeline.value.items.find(
      (candidate) => candidate.id === itemId,
    );
    if (
      !item ||
      item.kind !== "plan" ||
      item.slot.decisionState === "submitting"
    ) {
      return false;
    }
    const slot = item.slot;
    if (slot.approvalInvalidated) return false;
    commit(
      updateApprovalDecision(timeline.value, itemId, {
        decisionState: "submitting",
        optionId,
      }),
    );
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
      commit(
        updateApprovalDecision(timeline.value, itemId, {
          decisionState: "resolved",
          optionId,
          status: state,
        }),
      );
      return true;
    } catch (error) {
      commit(
        updateApprovalDecision(timeline.value, itemId, {
          decisionState: "error",
          optionId,
          errorMessage:
            error instanceof Error ? error.message : "Plan 处理失败",
        }),
      );
      return false;
    }
  }

  async function resumeSession(): Promise<boolean> {
    if (!timeline.value.taskId || !facade) return false;
    sendError.value = null;
    try {
      const result = await facade.resumeSession(timeline.value.taskId);
      if (result.success === "false") {
        sendError.value = desktopErrorMessage(result.error);
        return false;
      }
      await openTask(timeline.value.taskId, "新任务", true);
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
    queuedFollowUps,
    queueInterruptPending: computed(
      () => interruptFollowUp != null || cancelPending.value,
    ),
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
    workspaceAvailable,
    workspaceNotice,
    hasArtifacts,
    workspaceAttention,
    railNeeded,
    selectedModel,
    selectedReasoning,
    settingsPending,
    composerCapabilities,
    isRunning,
    attach,
    detach,
    refreshRailContext,
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
    enqueueFollowUp,
    editFollowUp,
    deleteFollowUp,
    sendFollowUpNow,
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

if (import.meta.hot) {
  import.meta.hot.accept(
    acceptHMRUpdate(useConversationStore, import.meta.hot),
  );
}
