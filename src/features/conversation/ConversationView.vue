<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import ErrorState from "../../shared/ui/ErrorState.vue";
import IconButton from "../../shared/ui/IconButton.vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import Skeleton from "../../shared/ui/Skeleton.vue";
import type { DesktopBridge, TaskId } from "../../bridge/types";
import { createConversationFacade } from "./conversation-facade";
import { pickImages } from "../../bridge/image-picker";
import { subscribeImageDrops } from "../../bridge/image-drop";
import { imageFileToBlobInput } from "./clipboard-images";
import { useConversationStore } from "./conversation-store";
import Composer from "./Composer.vue";
import ConversationHeader from "./ConversationHeader.vue";
import TimelineItemView from "./TimelineItemView.vue";
import TimelineVirtualList from "./TimelineVirtualList.vue";
import ArtifactPanel from "./ArtifactPanel.vue";
import WorktreePanel from "../worktrees/WorktreePanel.vue";
import type { SessionTimelineSnapshot } from "./types";
import { conversationThemeStyle } from "../../shared/theme/tokens";
import { applyTaskCenterHash } from "../task-center/hash-route";

const conversationTheme = conversationThemeStyle();

const props = defineProps<{
  bridge: DesktopBridge;
  taskId?: TaskId | string | null;
  /** Optional preloaded snapshot (fixture / tests). */
  snapshot?: SessionTimelineSnapshot | null;
  focusSeq?: number | null;
}>();

const store = useConversationStore();
const listRef = ref<InstanceType<typeof TimelineVirtualList> | null>(null);
const artifactPanel = ref<InstanceType<typeof ArtifactPanel> | null>(null);
const railForced = ref(false);
const focusedArtifactId = ref<string | null>(null);
const railTab = ref<"artifacts" | "workspace">("artifacts");
const previewUrls = ref<Record<string, string>>({});
const previewMissing = ref<Record<string, boolean>>({});
const previewRequested = new Set<string>();
const facade = createConversationFacade(props.bridge);

const imageAttachmentKey = computed(() =>
  store.items
    .flatMap((item) => (item.kind === "user" ? (item.attachments ?? []) : []))
    .filter((attachment) => attachment.mimeType.startsWith("image/"))
    .map((attachment) => `${attachment.artifactId}:${attachment.state}:${attachment.previewCapability}`)
    .join("|"),
);

watch(
  imageAttachmentKey,
  async () => {
    const taskId = (store.taskId ?? props.taskId) as TaskId | null | undefined;
    if (!taskId) return;
    for (const item of store.items) {
      if (item.kind !== "user") continue;
      for (const attachment of item.attachments ?? []) {
        if (!attachment.mimeType.startsWith("image/")) continue;
        if (previewRequested.has(attachment.artifactId)) continue;
        previewRequested.add(attachment.artifactId);
        if (
          attachment.state === "missing" ||
          attachment.state === "failed" ||
          attachment.state === "rejected" ||
          attachment.previewCapability === "none"
        ) {
          previewMissing.value = { ...previewMissing.value, [attachment.artifactId]: true };
          continue;
        }
        try {
          const result = await facade.previewArtifact(taskId, attachment.artifactId);
          if (result.success === "true" && result.data?.url) {
            previewUrls.value = {
              ...previewUrls.value,
              [attachment.artifactId]: result.data.url,
            };
          } else {
            previewMissing.value = { ...previewMissing.value, [attachment.artifactId]: true };
          }
        } catch {
          previewMissing.value = { ...previewMissing.value, [attachment.artifactId]: true };
        }
      }
    }
  },
  { immediate: true },
);

const railOpen = computed(() => store.railNeeded || railForced.value);

watch(
  () => [store.hasArtifacts, store.workspaceAttention] as const,
  ([hasArtifacts, attention]) => {
    if (hasArtifacts) railTab.value = "artifacts";
    else if (attention) railTab.value = "workspace";
  },
  { immediate: true },
);
const composerRegion = ref<HTMLElement | null>(null);
const nativeDropActive = ref(false);
let unsubscribeImageDrops: (() => void) | null = null;

const sessionKey = computed(
  () => String(store.sessionId ?? store.taskId ?? props.taskId ?? "none"),
);

const focusedId = computed(() => {
  if (props.focusSeq == null && store.focusEventSeq == null) return null;
  const seq = props.focusSeq ?? store.focusEventSeq;
  const item = store.items.find((i) => i.seq === seq);
  return item?.id ?? null;
});

onMounted(async () => {
  window.addEventListener("keydown", focusPendingApproval);
  await store.attach(props.bridge);
  if (props.snapshot) {
    store.openFromSnapshot(props.snapshot);
  } else if (props.taskId) {
    await store.openTask(props.taskId as TaskId);
  }
  await store.refreshRailContext();
  if (props.focusSeq != null) {
    store.setFocusEventSeq(props.focusSeq);
  }
  try {
    unsubscribeImageDrops = await subscribeImageDrops((event) => {
      if (event.type === "leave") {
        nativeDropActive.value = false;
        return;
      }
      const inside = isInsideComposer(event.clientX, event.clientY);
      nativeDropActive.value = inside && store.composerCapabilities.canSend;
      if (event.type === "drop" && inside) void onDroppedAttachments(event.paths);
    });
  } catch {
    // The native listener is optional in browser tests; the picker remains available.
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", focusPendingApproval);
  unsubscribeImageDrops?.();
  unsubscribeImageDrops = null;
  store.detach();
});

function focusPendingApproval(event: KeyboardEvent): void {
  if (!(event.ctrlKey && event.key === ".")) return;
  const pending = Array.from(
    document.querySelectorAll<HTMLElement>(
      '[data-testid="permission-slot"], [data-testid="plan-slot"]',
    ),
  ).filter((element) => element.querySelector("button:not(:disabled)"));
  const target = pending[pending.length - 1]?.querySelector<HTMLElement>(
    "[data-safe-default='true'], button:not(:disabled)",
  );
  if (target) {
    event.preventDefault();
    target.focus();
  }
}

watch(
  () => props.taskId,
  async (id) => {
    railForced.value = false;
    focusedArtifactId.value = null;
    previewUrls.value = {};
    previewMissing.value = {};
    previewRequested.clear();
    if (!id) return;
    if (props.snapshot && props.snapshot.taskId === id) {
      store.openFromSnapshot(props.snapshot);
      return;
    }
    await store.openTask(id as TaskId);
  },
);

watch(
  () => props.snapshot,
  (snap) => {
    railForced.value = false;
    focusedArtifactId.value = null;
    previewUrls.value = {};
    previewMissing.value = {};
    previewRequested.clear();
    if (snap) store.openFromSnapshot(snap);
  },
);

watch(
  () => props.focusSeq,
  (seq) => {
    store.setFocusEventSeq(seq ?? null);
  },
);

async function onSend(): Promise<void> {
  await store.sendMessage();
}

async function onCancel(): Promise<void> {
  await store.cancelTurn();
}

async function onResume(): Promise<void> {
  await store.resumeSession();
}

async function onAddAttachments(): Promise<void> {
  const result = await pickImages();
  if (result.error) store.sendError = result.error;
  else await store.importAttachmentPaths(result.paths);
}

async function onPasteImages(files: File[]): Promise<void> {
  const images = files.map((file) => ({
    file,
    displayName: file.name.trim() || `剪贴板图片.${file.type.split("/")[1] ?? "png"}`,
  }));
  try {
    const blobs: import("../../bridge/types").ArtifactBlobInput[] = [];
    for (const image of images) {
      blobs.push(await imageFileToBlobInput(image));
    }
    await store.importAttachmentBlobs(blobs);
  } catch (error) {
    store.sendError = error instanceof Error ? error.message : "剪贴板图片导入失败";
  }
}

function isInsideComposer(clientX: number, clientY: number): boolean {
  const bounds = composerRegion.value?.getBoundingClientRect();
  return Boolean(
    bounds &&
      clientX >= bounds.left &&
      clientX <= bounds.right &&
      clientY >= bounds.top &&
      clientY <= bounds.bottom,
  );
}

async function onDroppedAttachments(paths: string[]): Promise<void> {
  nativeDropActive.value = false;
  await store.importAttachmentPaths(paths);
}

async function onDropAttachments(paths: string[]): Promise<void> {
  await store.importAttachmentPaths(paths);
}

function onOpenArtifact(artifactId: string): void {
  railForced.value = true;
  focusedArtifactId.value = artifactId;
  railTab.value = "artifacts";
  void Promise.resolve().then(() => artifactPanel.value?.openArtifact(artifactId));
}

function onBack(): void {
  applyTaskCenterHash(null, null);
}

function firstLineOf(text: string): string {
  return text.split(/\r?\n/).find((line) => line.trim())?.trim() ?? "";
}

const turns = computed(() =>
  store.items
    .filter((item) => item.kind === "user")
    .map((item) => ({
      id: item.id,
      seq: item.seq,
      firstLine: firstLineOf(item.text) || (item.attachments?.length ? "（图片附件）" : "（空消息）"),
      timestamp: item.timestamp,
    })),
);

function onJumpTurn(seq: number): void {
  store.setFocusEventSeq(seq);
  listRef.value?.scrollToSeq(seq);
}

const TIME_GAP_MS = 5 * 60 * 1000;

function shouldShowRelativeTime(item: { id: string; timestamp: string }): boolean {
  const items = store.items;
  const index = items.findIndex((candidate) => candidate.id === item.id);
  if (index <= 0) return true;
  const previous = items[index - 1];
  if (!previous) return true;
  const delta = Date.parse(item.timestamp) - Date.parse(previous.timestamp);
  return Number.isFinite(delta) && delta >= TIME_GAP_MS;
}

function isThinkingDone(item: { id: string; kind: string; durationMs?: number }): boolean {
  if (item.kind !== "thinking") return false;
  if (item.durationMs != null) return true;
  const index = store.items.findIndex((candidate) => candidate.id === item.id);
  if (index < 0) return false;
  return store.items.slice(index + 1).some((candidate) => candidate.kind !== "thinking");
}
</script>

<template>
  <section
    class="conversation"
    data-testid="conversation-view"
    aria-label="对话与工具时间线"
    :style="conversationTheme"
  >
    <ConversationHeader
      :title="store.title"
      :status="store.status"
      :attempt="store.attempt"
      :needs-refresh="store.needsRefresh"
      :modes="store.modes"
      :selected-mode="store.selectedMode"
      :selected-workspace-strategy="store.workspaceStrategy"
      :settings-disabled="store.sendPending || store.settingsPending || store.isRunning"
      :turns="turns"
      @back="onBack"
      @jump-turn="onJumpTurn"
      @refresh="props.taskId && store.openTask(props.taskId as TaskId)"
      @resume="onResume"
      @update:mode="(mode, strategy) => void store.configureMode(mode, strategy)"
      @update:workspace-strategy="(strategy) => void store.configureWorkspaceStrategy(strategy)"
    />

    <p
      v-if="store.workspaceNotice"
      class="workspace-notice"
      role="status"
      data-testid="conversation-workspace-notice"
    >
      {{ store.workspaceNotice }}
    </p>

    <div class="content-layout" :class="{ 'has-rail': railOpen }">
      <div class="body">
        <div v-if="store.loadState === 'loading'" class="state-pad">
          <Skeleton />
          <Skeleton />
        </div>
        <ErrorState
          v-else-if="store.loadState === 'error'"
          title="会话加载失败"
          :detail="store.errorMessage ?? '未知错误'"
        />
        <div
          v-else-if="store.items.length === 0"
          class="empty-conversation"
          data-testid="empty-conversation"
          role="status"
        >
          <h2>把目标发给智能体</h2>
          <p>下方输入；需要时用 / 看快捷指令，或点回形针加图</p>
        </div>
        <TimelineVirtualList
          v-else
          ref="listRef"
          :items="store.items"
          :session-key="sessionKey"
          :focus-seq="props.focusSeq ?? store.focusEventSeq"
          :item-height="128"
        >
          <template #default="{ item }">
            <TimelineItemView
              :item="item"
              :focused="item.id === focusedId"
              :show-time="shouldShowRelativeTime(item)"
              :thinking-done="isThinkingDone(item)"
              :preview-urls="previewUrls"
              :preview-missing="previewMissing"
              @toggle-tool="store.toggleTool"
              @toggle-thinking="store.toggleThinking"
              @resolve-permission="store.resolvePermission"
              @resolve-plan="store.resolvePlan"
              @open-artifact="onOpenArtifact"
            />
          </template>
        </TimelineVirtualList>
      </div>
      <aside
        v-if="railOpen"
        class="conversation-rail"
        data-testid="conversation-rail"
        aria-label="对话侧栏"
      >
        <div class="rail-tabs" role="tablist" aria-label="侧栏内容">
          <button
            type="button"
            role="tab"
            data-testid="rail-tab-artifacts"
            :aria-selected="railTab === 'artifacts'"
            :class="{ on: railTab === 'artifacts' }"
            @click="railTab = 'artifacts'"
          >
            图片与结果
          </button>
          <button
            type="button"
            role="tab"
            data-testid="rail-tab-workspace"
            :aria-selected="railTab === 'workspace'"
            :class="{ on: railTab === 'workspace' }"
            @click="railTab = 'workspace'"
          >
            工作区
          </button>
        </div>
        <ArtifactPanel
          v-show="railTab === 'artifacts'"
          ref="artifactPanel"
          :bridge="props.bridge"
          :task-id="store.taskId"
          :refresh-key="store.artifactRevision"
          :focus-artifact-id="focusedArtifactId"
        />
        <WorktreePanel
          v-if="store.taskId && railTab === 'workspace'"
          :bridge="props.bridge"
          :task-id="store.taskId"
        />
      </aside>
    </div>

    <div v-if="store.queuedFollowUps.length" class="queue-bar" data-testid="queue-bar">
      <article
        v-for="item in store.queuedFollowUps"
        :key="item.id"
        class="queue-item"
        data-testid="queue-item"
      >
        <p class="queue-text">{{ item.text || "（图片附件）" }}</p>
        <div class="queue-actions">
          <IconButton label="编辑" data-testid="queue-edit" @click="store.editFollowUp(item.id)">
            <NamedIcon name="pencil" :size="14" />
          </IconButton>
          <IconButton label="立即发送" data-testid="queue-send-now" @click="void store.sendFollowUpNow(item.id)">
            <NamedIcon name="play" :size="14" />
          </IconButton>
          <IconButton label="删除" data-testid="queue-delete" @click="store.deleteFollowUp(item.id)">
            <NamedIcon name="x" :size="14" />
          </IconButton>
        </div>
      </article>
    </div>
    <Composer
      ref="composerRegion"
      :model-value="store.draft"
      :capabilities="store.composerCapabilities"
      :send-error="store.sendError"
      :send-pending="store.sendPending"
      :attachment-pending="store.attachmentPending"
      :attachments="store.attachments"
      :drop-active="nativeDropActive"
      :slash-commands="store.slashCommands"
      :models="store.models"
      :selected-model="store.selectedModel"
      :selected-reasoning="store.selectedReasoning"
      :settings-locked="store.sendPending || store.settingsPending || store.isRunning"
      @update:model-value="store.setDraft"
      @update:model="(model: string | null) => void store.configureModel(model)"
      @update:reasoning="(reasoning) => void store.configureReasoning(reasoning)"
      @send="onSend"
      @queue="store.enqueueFollowUp"
      @cancel="onCancel"
      @add-attachments="onAddAttachments"
      @drop-attachments="onDropAttachments"
      @paste-images="onPasteImages"
      @remove-attachment="store.removeAttachment"
    />
  </section>
</template>

<style scoped>
.conversation {
  display: grid;
  grid-template-rows: auto auto 1fr auto auto;
  height: 100%;
  min-height: 0;
  color: var(--ctp-text);
  background: var(--ctp-base);
  /* Recompute overlays against remapped --ctp-* so they are not leftover Mocha mixes. */
  --overlay-hover: color-mix(in srgb, var(--ctp-surface1) 40%, transparent);
  --overlay-active: color-mix(in srgb, var(--ctp-mauve) 16%, transparent);
  --overlay-info: color-mix(in srgb, var(--ctp-blue) 12%, transparent);
  --overlay-info-solid: color-mix(in srgb, var(--ctp-blue) 12%, var(--ctp-base));
  --overlay-success: color-mix(in srgb, var(--ctp-green) 12%, transparent);
  --overlay-danger: color-mix(in srgb, var(--ctp-red) 14%, transparent);
  --overlay-warning: color-mix(in srgb, var(--ctp-yellow) 16%, var(--ctp-mantle));
  --overlay-menu-active: color-mix(in srgb, var(--ctp-blue) 18%, var(--ctp-surface0));
  --border-tone-info: color-mix(in srgb, var(--ctp-blue) 40%, var(--ctp-surface1));
  --border-tone-success: color-mix(in srgb, var(--ctp-green) 40%, var(--ctp-surface1));
  --border-tone-warning: color-mix(in srgb, var(--ctp-yellow) 50%, var(--ctp-surface1));
  --border-tone-danger: color-mix(in srgb, var(--ctp-red) 55%, var(--ctp-surface1));
}
.workspace-notice {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  color: var(--ctp-yellow);
  background: var(--overlay-warning);
  border-bottom: 1px solid var(--ctp-surface0);
  font-size: var(--font-small);
}
.content-layout { display: grid; grid-template-columns: minmax(0, 1fr); min-height: 0; overflow: hidden; }
.content-layout.has-rail { grid-template-columns: minmax(0, 1fr) minmax(260px, 340px); }
.conversation-rail {
  display: grid;
  grid-template-rows: auto 1fr;
  min-width: 0;
  overflow: hidden;
  background: color-mix(in srgb, var(--ctp-mauve) 6%, var(--ctp-mantle));
  border-left: 1px solid var(--ctp-surface0);
}
.rail-tabs {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--ctp-surface0);
}
.rail-tabs button {
  padding: 4px 10px;
  border: 0;
  border-radius: var(--radius-control);
  color: var(--ctp-subtext0);
  background: transparent;
  cursor: pointer;
}
.rail-tabs button.on {
  color: var(--ctp-text);
  background: color-mix(in srgb, var(--ctp-mauve) 18%, transparent);
}
.body {
  min-height: 0;
  overflow: hidden;
}
.state-pad {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
}
.empty-conversation {
  max-width: 36rem;
  padding: var(--space-6) var(--space-4);
  color: var(--ctp-subtext0);
}
.empty-conversation h2 {
  margin: 0 0 var(--space-2);
  color: var(--ctp-text);
  font-size: var(--heading-panel);
  font-weight: var(--font-weight-semibold);
}
.empty-conversation p {
  margin: 0;
}
.queue-bar {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3) 0;
}
.queue-item {
  display: flex;
  gap: var(--space-2);
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface0);
  border-radius: var(--radius-control);
}
.queue-text {
  margin: 0;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.queue-actions {
  display: flex;
  flex-shrink: 0;
  gap: var(--space-1);
}
@media (max-width: 860px) {
  .content-layout,
  .content-layout.has-rail { grid-template-columns: 1fr; grid-template-rows: minmax(220px, 1fr) auto; }
}
</style>
