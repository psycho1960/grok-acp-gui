<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { TimelineItem } from "./types";
import {
  isNearBottom,
  jumpToBottom,
  loadScrollAnchor,
  onItemsAppended,
  onUserScroll,
  saveScrollAnchor,
  type ScrollAnchor,
} from "./scroll";

const props = withDefaults(
  defineProps<{
    items: readonly TimelineItem[];
    /** Estimated row height for virtualization. */
    itemHeight?: number;
    overscan?: number;
    sessionKey?: string;
    focusSeq?: number | null;
  }>(),
  {
    itemHeight: 120,
    overscan: 8,
    sessionKey: "default",
    focusSeq: null,
  },
);

const emit = defineEmits<{
  "update:anchor": [anchor: ScrollAnchor];
}>();

const root = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(400);
const anchor = ref<ScrollAnchor>(loadScrollAnchor(props.sessionKey));
let prevCount = props.items.length;

const totalHeight = computed(() => props.items.length * props.itemHeight);

const range = computed(() => {
  const start = Math.max(
    0,
    Math.floor(scrollTop.value / props.itemHeight) - props.overscan,
  );
  const visible =
    Math.ceil(viewportHeight.value / props.itemHeight) + props.overscan * 2;
  const end = Math.min(props.items.length, start + visible);
  return { start, end };
});

const windowItems = computed(() => {
  const { start, end } = range.value;
  return props.items.slice(start, end).map((item, offset) => {
    const index = start + offset;
    return {
      item,
      index,
      key: item.id,
      top: index * props.itemHeight,
    };
  });
});

const renderedCount = computed(() => windowItems.value.length);

function persist(): void {
  saveScrollAnchor(props.sessionKey, anchor.value);
  emit("update:anchor", anchor.value);
}

function onScroll(): void {
  if (!root.value) return;
  scrollTop.value = root.value.scrollTop;
  anchor.value = onUserScroll(
    anchor.value,
    root.value.scrollTop,
    root.value.clientHeight,
    root.value.scrollHeight,
  );
  persist();
}

function scrollToBottom(smooth = false): void {
  if (!root.value) return;
  const top = Math.max(0, root.value.scrollHeight - root.value.clientHeight);
  if (smooth && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    root.value.scrollTo({ top, behavior: "smooth" });
  } else {
    root.value.scrollTop = top;
  }
  scrollTop.value = top;
  anchor.value = jumpToBottom(anchor.value);
  persist();
}

function scrollToSeq(seq: number): void {
  const idx = props.items.findIndex((i) => i.seq === seq);
  if (idx < 0 || !root.value) return;
  const top = idx * props.itemHeight;
  root.value.scrollTop = top;
  scrollTop.value = top;
  anchor.value = {
    ...anchor.value,
    stickToBottom: false,
    scrollTop: top,
    anchorEventKey: props.items[idx]?.eventKey,
  };
  persist();
}

watch(
  () => props.items.length,
  async (len) => {
    const appended = len - prevCount;
    prevCount = len;
    if (appended > 0) {
      anchor.value = onItemsAppended(anchor.value, appended);
      persist();
      if (anchor.value.stickToBottom) {
        await nextTick();
        scrollToBottom(false);
      }
    }
  },
);

watch(
  () => props.sessionKey,
  (key) => {
    anchor.value = loadScrollAnchor(key);
    prevCount = props.items.length;
    nextTick(() => {
      if (anchor.value.stickToBottom) scrollToBottom(false);
      else if (root.value) root.value.scrollTop = anchor.value.scrollTop;
    });
  },
);

watch(
  () => props.focusSeq,
  (seq) => {
    if (seq != null) nextTick(() => scrollToSeq(seq));
  },
);

let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  if (!root.value) return;
  viewportHeight.value = root.value.clientHeight || 400;
  resizeObserver = new ResizeObserver(() => {
    if (root.value) viewportHeight.value = root.value.clientHeight || 400;
  });
  resizeObserver.observe(root.value);
  if (props.focusSeq != null) {
    scrollToSeq(props.focusSeq);
  } else if (anchor.value.stickToBottom) {
    scrollToBottom(false);
  }
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  persist();
});

defineExpose({
  root,
  scrollToBottom,
  scrollToSeq,
  anchor,
  range,
  renderedCount,
  totalHeight,
  isNearBottom: () =>
    root.value
      ? isNearBottom(
          root.value.scrollTop,
          root.value.clientHeight,
          root.value.scrollHeight,
        )
      : true,
});
</script>

<template>
  <div class="virtual-wrap">
    <div
      ref="root"
      class="virtual-list"
      role="list"
      aria-label="对话时间线"
      data-testid="conversation-virtual-list"
      @scroll="onScroll"
    >
      <div
        class="virtual-spacer"
        data-testid="conversation-virtual-spacer"
        :style="{ height: `${totalHeight}px` }"
      >
        <div
          v-for="row in windowItems"
          :key="row.key"
          class="virtual-row"
          :data-index="row.index"
          :style="{
            height: `${itemHeight}px`,
            transform: `translateY(${row.top}px)`,
          }"
        >
          <slot :item="row.item" :index="row.index" />
        </div>
      </div>
    </div>
    <button
      v-if="!anchor.stickToBottom"
      type="button"
      class="jump-bottom"
      data-testid="jump-to-bottom"
      @click="scrollToBottom(true)"
    >
      回到底部
      <span v-if="anchor.unreadCount > 0" class="unread" data-testid="unread-count">
        {{ anchor.unreadCount }}
      </span>
    </button>
  </div>
</template>

<style scoped>
.virtual-wrap {
  position: relative;
  height: 100%;
  min-height: 200px;
}
.virtual-list {
  position: relative;
  overflow: auto;
  height: 100%;
}
.virtual-spacer {
  position: relative;
  width: 100%;
}
.virtual-row {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  box-sizing: border-box;
  overflow: hidden;
}
.jump-bottom {
  position: absolute;
  right: var(--space-3);
  bottom: var(--space-3);
  z-index: 2;
  min-height: var(--control-min-size);
  padding: 0 var(--space-3);
  display: inline-flex;
  gap: var(--space-2);
  align-items: center;
  color: var(--ctp-crust);
  background: var(--ctp-mauve);
  border: none;
  border-radius: 999px;
  cursor: pointer;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
}
.unread {
  min-width: 20px;
  padding: 0 6px;
  border-radius: 999px;
  background: var(--ctp-crust);
  color: var(--ctp-mauve);
  font-size: var(--font-small);
  text-align: center;
}
</style>
