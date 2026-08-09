<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import EmptyState from "../../shared/ui/EmptyState.vue";
import ErrorState from "../../shared/ui/ErrorState.vue";
import Skeleton from "../../shared/ui/Skeleton.vue";
import type { DesktopBridge, TaskId } from "../../bridge/types";
import { pickImages } from "../../bridge/image-picker";
import { subscribeImageDrops } from "../../bridge/image-drop";
import { imageFileToBlobInput } from "./clipboard-images";
import { useConversationStore } from "./conversation-store";
import Composer from "./Composer.vue";
import ConversationHeader from "./ConversationHeader.vue";
import TimelineItemView from "./TimelineItemView.vue";
import TimelineVirtualList from "./TimelineVirtualList.vue";
import ArtifactPanel from "./ArtifactPanel.vue";
import type { SessionTimelineSnapshot } from "./types";

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
  artifactPanel.value?.openArtifact(artifactId);
}
</script>

<template>
  <section class="conversation" data-testid="conversation-view" aria-label="对话与工具时间线">
    <ConversationHeader
      :title="store.title"
      :status="store.status"
      :attempt="store.attempt"
      :can-cancel="store.composerCapabilities.canCancel"
      :needs-refresh="store.needsRefresh"
      :modes="store.modes"
      :models="store.models"
      :selected-mode="store.selectedMode"
      :selected-workspace-strategy="store.workspaceStrategy"
      :selected-model="store.selectedModel"
      :selected-reasoning="store.selectedReasoning"
      :settings-disabled="store.sendPending || store.settingsPending || store.isRunning"
      @cancel="onCancel"
      @refresh="props.taskId && store.openTask(props.taskId as TaskId)"
      @resume="onResume"
      @update:mode="(mode, strategy) => void store.configureMode(mode, strategy)"
      @update:workspace-strategy="(strategy) => void store.configureWorkspaceStrategy(strategy)"
      @update:model="(model: string | null) => void store.configureModel(model)"
      @update:reasoning="(reasoning) => void store.configureReasoning(reasoning)"
    />

    <p
      v-if="store.workspaceNotice"
      class="workspace-notice"
      role="status"
      data-testid="conversation-workspace-notice"
    >
      {{ store.workspaceNotice }}
    </p>

    <div class="content-layout">
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
        <EmptyState
          v-else-if="store.items.length === 0"
          title="还没有消息"
          detail="在下方输入并发送，开始与智能体对话。"
        />
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
              @toggle-tool="store.toggleTool"
              @toggle-thinking="store.toggleThinking"
              @resolve-permission="store.resolvePermission"
              @resolve-plan="store.resolvePlan"
              @open-artifact="onOpenArtifact"
            />
          </template>
        </TimelineVirtualList>
      </div>
      <ArtifactPanel
        ref="artifactPanel"
        :bridge="props.bridge"
        :task-id="store.taskId"
        :refresh-key="store.artifactRevision"
      />
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
      @update:model-value="store.setDraft"
      @send="onSend"
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
  grid-template-rows: auto auto 1fr auto;
  height: 100%;
  min-height: 0;
  background: var(--ctp-base);
}
.workspace-notice {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  color: var(--ctp-yellow);
  background: color-mix(in srgb, var(--ctp-yellow) 10%, var(--ctp-base));
  border-bottom: 1px solid var(--ctp-surface0);
  font-size: var(--font-small);
}
.content-layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(260px, 340px); min-height: 0; overflow: hidden; }
.body {
  min-height: 0;
  overflow: hidden;
}
.state-pad {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
}
@media (max-width: 860px) { .content-layout { grid-template-columns: 1fr; grid-template-rows: minmax(220px, 1fr) auto; } }
</style>
