<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import EmptyState from "../../shared/ui/EmptyState.vue";
import ErrorState from "../../shared/ui/ErrorState.vue";
import Skeleton from "../../shared/ui/Skeleton.vue";
import type { DesktopBridge, TaskId } from "../../bridge/types";
import { useConversationStore } from "./conversation-store";
import Composer from "./Composer.vue";
import ConversationHeader from "./ConversationHeader.vue";
import TimelineItemView from "./TimelineItemView.vue";
import TimelineVirtualList from "./TimelineVirtualList.vue";
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
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", focusPendingApproval);
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
</script>

<template>
  <section class="conversation" data-testid="conversation-view" aria-label="对话与工具时间线">
    <ConversationHeader
      :title="store.title"
      :status="store.status"
      :attempt="store.attempt"
      :can-cancel="store.composerCapabilities.canCancel"
      :needs-refresh="store.needsRefresh"
      @cancel="onCancel"
      @refresh="props.taskId && store.openTask(props.taskId as TaskId)"
      @resume="onResume"
    />

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
        detail="在下方输入并发送，开始与 Agent 对话。"
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
          />
        </template>
      </TimelineVirtualList>
    </div>

    <Composer
      :model-value="store.draft"
      :capabilities="store.composerCapabilities"
      :send-error="store.sendError"
      :send-pending="store.sendPending"
      @update:model-value="store.setDraft"
      @send="onSend"
      @cancel="onCancel"
    />
  </section>
</template>

<style scoped>
.conversation {
  display: grid;
  grid-template-rows: auto 1fr auto;
  height: 100%;
  min-height: 0;
  background: var(--ctp-base);
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
</style>
