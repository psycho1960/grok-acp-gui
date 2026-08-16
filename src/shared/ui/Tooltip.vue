<script setup lang="ts">
import { cloneVNode, computed, defineComponent, useId, type PropType, type VNode } from "vue";

const props = withDefaults(
  defineProps<{ text: string; placement?: "top" | "bottom" }>(),
  { placement: "top" },
);
const slots = defineSlots<{ default?: () => VNode[] }>();
const tooltipId = useId();
const trigger = computed(() => {
  const child = slots.default?.()[0];
  if (!child) return undefined;
  const currentDescription = child.props?.["aria-describedby"];
  const describedBy = [currentDescription, tooltipId].filter(Boolean).join(" ");
  return cloneVNode(child, { "aria-describedby": describedBy });
});
const RenderVNode = defineComponent({
  props: { node: { type: Object as PropType<VNode>, required: true } },
  setup(renderProps) {
    return () => renderProps.node;
  },
});
</script>

<template>
  <span class="tooltip" :class="`is-${placement}`">
    <RenderVNode v-if="trigger" :node="trigger" />
    <span :id="tooltipId" class="tip" role="tooltip">{{ props.text }}</span>
  </span>
</template>

<style scoped>
.tooltip {
  position: relative;
  display: inline-flex;
}
.tip {
  position: absolute;
  z-index: 5;
  bottom: calc(100% + 6px);
  left: 50%;
  width: max-content;
  max-width: 220px;
  padding: 4px 8px;
  color: var(--ctp-text);
  pointer-events: none;
  visibility: hidden;
  opacity: 0;
  background: var(--ctp-crust);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  font-size: var(--font-small);
  transform: translateX(-50%);
  transition:
    opacity var(--motion-fast) ease,
    visibility 0s linear var(--motion-fast);
  transition-delay: 0ms, 0ms;
}
.tooltip.is-bottom .tip {
  top: calc(100% + 6px);
  bottom: auto;
}
/* Delay show ~300ms so tooltips don't flash while moving the pointer. */
.tooltip:hover .tip,
.tooltip:focus-within .tip {
  visibility: visible;
  opacity: 1;
  transition-delay: 300ms, 300ms;
}
</style>
