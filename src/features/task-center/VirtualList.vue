<script setup lang="ts" generic="T">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    items: readonly T[];
    itemHeight: number;
    /** Extra rows rendered above/below the viewport. */
    overscan?: number;
    ariaLabel?: string;
  }>(),
  { overscan: 6, ariaLabel: "任务列表" },
);

const root = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(400);

const totalHeight = computed(() => props.items.length * props.itemHeight);

const range = computed(() => {
  const start = Math.max(
    0,
    Math.floor(scrollTop.value / props.itemHeight) - props.overscan,
  );
  const visible = Math.ceil(viewportHeight.value / props.itemHeight) + props.overscan * 2;
  const end = Math.min(props.items.length, start + visible);
  return { start, end };
});

const windowItems = computed(() => {
  const { start, end } = range.value;
  return props.items.slice(start, end).map((item, offset) => ({
    item,
    index: start + offset,
    top: (start + offset) * props.itemHeight,
  }));
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
  () => props.items.length,
  () => {
    if (root.value && scrollTop.value > totalHeight.value) {
      root.value.scrollTop = Math.max(0, totalHeight.value - viewportHeight.value);
      scrollTop.value = root.value.scrollTop;
    }
  },
);
</script>

<template>
  <div
    ref="root"
    class="virtual-list"
    role="list"
    :aria-label="ariaLabel"
    @scroll="onScroll"
  >
    <div class="virtual-list-spacer" :style="{ height: `${totalHeight}px` }">
      <div
        v-for="row in windowItems"
        :key="row.index"
        class="virtual-list-row"
        role="listitem"
        :style="{
          height: `${itemHeight}px`,
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
