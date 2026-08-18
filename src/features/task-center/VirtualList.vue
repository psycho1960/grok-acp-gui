<script setup lang="ts" generic="T">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    items: readonly T[];
    itemHeight: number;
    /** Per-item height. Falls back to itemHeight for fixed-height lists. */
    getItemHeight?: (item: T, index: number) => number;
    /** Extra rows rendered above/below the viewport. */
    overscan?: number;
    ariaLabel?: string;
    /** Stable key for each item — prefer id over index. */
    getKey?: (item: T, index: number) => string | number;
  }>(),
  {
    overscan: 6,
    ariaLabel: "任务列表",
    getKey: undefined,
    getItemHeight: undefined,
  },
);

const root = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(400);

const rows = computed(() => {
  let top = 0;
  return props.items.map((item, index) => {
    const candidate = props.getItemHeight?.(item, index) ?? props.itemHeight;
    const height = Number.isFinite(candidate) && candidate > 0 ? candidate : props.itemHeight;
    const row = {
      item,
      index,
      key: props.getKey ? props.getKey(item, index) : index,
      top,
      height,
      bottom: top + height,
    };
    top = row.bottom;
    return row;
  });
});

const totalHeight = computed(() => rows.value[rows.value.length - 1]?.bottom ?? 0);

function firstRowEndingAfter(offset: number): number {
  let low = 0;
  let high = rows.value.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (rows.value[middle].bottom <= offset) low = middle + 1;
    else high = middle;
  }
  return low;
}

function firstRowStartingAtOrAfter(offset: number): number {
  let low = 0;
  let high = rows.value.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (rows.value[middle].top < offset) low = middle + 1;
    else high = middle;
  }
  return low;
}

const range = computed(() => {
  const visibleStart = firstRowEndingAfter(scrollTop.value);
  const visibleEnd = firstRowStartingAtOrAfter(scrollTop.value + viewportHeight.value);
  const start = Math.max(0, visibleStart - props.overscan);
  const end = Math.min(rows.value.length, visibleEnd + props.overscan);
  return { start, end };
});

const windowItems = computed(() => {
  const { start, end } = range.value;
  return rows.value.slice(start, end);
});

function onScroll(): void {
  if (root.value) scrollTop.value = root.value.scrollTop;
}

let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  if (!root.value) return;
  viewportHeight.value = root.value.clientHeight || 400;
  resizeObserver = new ResizeObserver(() => {
    if (root.value) viewportHeight.value = root.value.clientHeight || 400;
  });
  resizeObserver.observe(root.value);
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
});

watch(
  totalHeight,
  () => {
    if (root.value && scrollTop.value > totalHeight.value) {
      root.value.scrollTop = Math.max(0, totalHeight.value - viewportHeight.value);
      scrollTop.value = root.value.scrollTop;
    }
  },
);

defineExpose({
  root,
  scrollTop,
  range,
  totalHeight,
});
</script>

<template>
  <div
    ref="root"
    class="virtual-list"
    role="list"
    :aria-label="ariaLabel"
    :aria-rowcount="items.length"
    data-testid="virtual-list"
    @scroll="onScroll"
  >
    <div
      class="virtual-list-spacer"
      data-testid="virtual-list-spacer"
      :style="{ height: `${totalHeight}px` }"
    >
      <div
        v-for="row in windowItems"
        :key="row.key"
        class="virtual-list-row"
        role="listitem"
        :data-index="row.index"
        :aria-posinset="row.index + 1"
        :aria-setsize="items.length"
        :style="{
          height: `${row.height}px`,
          transform: `translateY(${row.top}px)`,
        }"
      >
        <slot :item="row.item" :index="row.index" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.virtual-list {
  position: relative;
  overflow: auto;
  height: 100%;
  min-height: 200px;
}
.virtual-list-spacer {
  position: relative;
  width: 100%;
}
.virtual-list-row {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  box-sizing: border-box;
}
</style>
