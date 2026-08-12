<script setup lang="ts">
/**
 * Inline SVG icon shell. Paths come from named icon components or raw path slots.
 * Decorative by default (aria-hidden); pass label for meaningful standalone icons.
 */
withDefaults(
  defineProps<{
    size?: number | string;
    strokeWidth?: number | string;
    label?: string;
  }>(),
  { size: 16, strokeWidth: 2, label: undefined },
);
</script>

<template>
  <svg
    class="ui-icon"
    xmlns="http://www.w3.org/2000/svg"
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    :stroke-width="strokeWidth"
    stroke-linecap="round"
    stroke-linejoin="round"
    :role="label ? 'img' : undefined"
    :aria-label="label"
    :aria-hidden="label ? undefined : true"
    focusable="false"
  >
    <slot />
  </svg>
</template>

<style scoped>
.ui-icon {
  display: inline-block;
  flex-shrink: 0;
  vertical-align: middle;
}
</style>
