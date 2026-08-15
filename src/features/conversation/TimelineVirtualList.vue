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
let pendingRestoredScrollTop: number | null = null;
let restoreReleaseFrame: number | null = null;
const layoutRevision = ref(0);
const measuredHeights = new Map<string, number>();

const layout = computed(() => {
  const revision = layoutRevision.value;
  const tops: number[] = [];
  const heights: number[] = [];
  let top = 0;
  for (const item of props.items) {
    tops.push(top);
    const height = Math.max(
      props.itemHeight,
      measuredHeights.get(item.id) ?? props.itemHeight,
    );
    heights.push(height);
    top += height;
  }
  return { tops, heights, total: top, revision };
});

const totalHeight = computed(() => layout.value.total);

function indexAt(offset: number): number {
  const tops = layout.value.tops;
  let low = 0;
  let high = tops.length;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if ((tops[mid] ?? 0) < offset) low = mid + 1;
    else high = mid;
  }
  return Math.max(0, Math.min(tops.length - 1, low - 1));
}

const range = computed(() => {
  if (props.items.length === 0) return { start: 0, end: 0 };
  const start = Math.max(0, indexAt(scrollTop.value) - props.overscan);
  const end = Math.min(
    props.items.length,
    indexAt(scrollTop.value + viewportHeight.value) + props.overscan + 2,
  );
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
      top: layout.value.tops[index] ?? 0,
      height: layout.value.heights[index] ?? props.itemHeight,
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
  if (pendingRestoredScrollTop != null) return;
  const topIndex = indexAt(root.value.scrollTop);
  const itemTop = layout.value.tops[topIndex] ?? 0;
  const next = onUserScroll(
    anchor.value,
    root.value.scrollTop,
    root.value.clientHeight,
    root.value.scrollHeight,
  );
  anchor.value = {
    ...next,
    anchorEventKey: props.items[topIndex]?.eventKey,
    anchorOffsetPx: Math.max(0, root.value.scrollTop - itemTop),
  };
  persist();
}

function restoreSavedPosition(): void {
  if (!root.value || anchor.value.stickToBottom) return;
  let top = anchor.value.scrollTop;
  if (anchor.value.anchorEventKey) {
    const index = props.items.findIndex(
      (item) => item.eventKey === anchor.value.anchorEventKey,
    );
    if (index >= 0) {
      top =
        (layout.value.tops[index] ?? top) +
        Math.max(0, anchor.value.anchorOffsetPx ?? 0);
    }
  }
  // Row measurements can queue several scroll events while the virtual layout
  // settles. Protect the whole batch so none of those programmatic events can
  // reinterpret the saved reading position as a user scroll at the bottom.
  pendingRestoredScrollTop = top;
  root.value.scrollTop = top;
  scrollTop.value = root.value.scrollTop;
  pendingRestoredScrollTop = root.value.scrollTop;
  if (restoreReleaseFrame != null) cancelAnimationFrame(restoreReleaseFrame);
  restoreReleaseFrame = requestAnimationFrame(() => {
    restoreReleaseFrame = requestAnimationFrame(() => {
      pendingRestoredScrollTop = null;
      restoreReleaseFrame = null;
    });
  });
}

function scrollToBottom(smooth = false): void {
  if (!root.value) return;
  pendingRestoredScrollTop = null;
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

function scrollToIndex(idx: number): void {
  if (idx < 0 || !root.value) return;
  pendingRestoredScrollTop = null;
  const top = layout.value.tops[idx] ?? idx * props.itemHeight;
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

function scrollToSeq(seq: number): void {
  scrollToIndex(props.items.findIndex((item) => item.seq === seq));
}

function scrollToId(id: string): void {
  scrollToIndex(props.items.findIndex((item) => item.id === id));
}

function visibleRevision(): string {
  const last = props.items[props.items.length - 1];
  if (!last) return "0";
  if (last.kind === "assistant") {
    return `${props.items.length}:${last.id}:${last.seq}:${last.text.length}:${last.streaming}`;
  }
  if (last.kind === "tool") {
    return `${props.items.length}:${last.id}:${last.seq}:${last.tool.phase}:${last.tool.result.summary}`;
  }
  return `${props.items.length}:${last.id}:${last.seq}:${last.kind}`;
}

watch(
  visibleRevision,
  async (revision, previous) => {
    const len = props.items.length;
    const appended = len - prevCount;
    prevCount = len;
    if (appended > 0) {
      anchor.value = onItemsAppended(anchor.value, appended);
      persist();
      if (anchor.value.stickToBottom) {
        await nextTick();
        scrollToBottom(false);
      } else {
        await nextTick();
        restoreSavedPosition();
      }
    } else if (previous != null && revision !== previous) {
      anchor.value = onItemsAppended(anchor.value, 1);
      persist();
      if (anchor.value.stickToBottom) {
        await nextTick();
        scrollToBottom(false);
      } else {
        await nextTick();
        restoreSavedPosition();
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
      else restoreSavedPosition();
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
let rowResizeObserver: ResizeObserver | null = null;

function measureRow(element: HTMLElement, id: string): void {
  const height = Math.ceil(element.getBoundingClientRect().height);
  if (height <= 0 || measuredHeights.get(id) === height) return;
  measuredHeights.set(id, height);
  layoutRevision.value += 1;
  if (anchor.value.stickToBottom) nextTick(() => scrollToBottom(false));
  else nextTick(restoreSavedPosition);
}

function setRowElement(element: unknown, id: string): void {
  if (!(element instanceof HTMLElement)) return;
  element.dataset.rowKey = id;
  rowResizeObserver?.observe(element);
  measureRow(element, id);
}

onMounted(() => {
  if (!root.value) return;
  viewportHeight.value = root.value.clientHeight || 400;
  resizeObserver = new ResizeObserver(() => {
    if (root.value) viewportHeight.value = root.value.clientHeight || 400;
  });
  resizeObserver.observe(root.value);
  rowResizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      const element = entry.target as HTMLElement;
      const id = element.dataset.rowKey;
      if (id) measureRow(element, id);
    }
  });
  root.value.querySelectorAll<HTMLElement>(".virtual-row").forEach((element) => {
    const id = element.dataset.rowKey;
    if (id) rowResizeObserver?.observe(element);
  });
  if (props.focusSeq != null) {
    scrollToSeq(props.focusSeq);
  } else if (anchor.value.stickToBottom) {
    scrollToBottom(false);
  } else {
    restoreSavedPosition();
  }
});

onBeforeUnmount(() => {
  if (restoreReleaseFrame != null) cancelAnimationFrame(restoreReleaseFrame);
  resizeObserver?.disconnect();
  rowResizeObserver?.disconnect();
  persist();
});

defineExpose({
  root,
  scrollToBottom,
  scrollToSeq,
  scrollToId,
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
          :ref="(element) => setRowElement(element, row.item.id)"
          :key="row.key"
          class="virtual-row"
          :data-index="row.index"
          :data-row-key="row.item.id"
          :style="{
            minHeight: `${itemHeight}px`,
            transform: `translateY(${row.top}px)`,
          }"
        >
          <slot :item="row.item" :index="row.index" />
        </div>
      </div>
    </div>
    <button
      type="button"
      class="jump-bottom"
      :class="{
        visible: !anchor.stickToBottom,
        'has-unread': anchor.unreadCount > 0,
      }"
      data-testid="jump-to-bottom"
      :aria-hidden="anchor.stickToBottom ? 'true' : undefined"
      :tabindex="anchor.stickToBottom ? -1 : 0"
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
  box-shadow: var(--shadow-md);
  opacity: 0;
  visibility: hidden;
  transform: translateY(6px);
  pointer-events: none;
  transition:
    opacity var(--motion-fast) ease,
    transform var(--motion-fast) ease,
    visibility 0s linear var(--motion-fast);
}
.jump-bottom.visible {
  opacity: 0.92;
  visibility: visible;
  transform: translateY(0);
  pointer-events: auto;
  transition-delay: 0s;
}
.jump-bottom.has-unread {
  opacity: 1;
}
.jump-bottom.visible:hover,
.jump-bottom.visible:focus-visible {
  opacity: 1;
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
