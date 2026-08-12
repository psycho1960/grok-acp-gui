<script setup lang="ts">
import Icon from "./Icon.vue";
import type { IconName } from "./icons";

withDefaults(
  defineProps<{
    name: IconName;
    size?: number | string;
    strokeWidth?: number | string;
    label?: string;
  }>(),
  { size: 16, strokeWidth: 2, label: undefined },
);
</script>

<template>
  <Icon :size="size" :stroke-width="strokeWidth" :label="label">
    <template v-if="name === 'circle'">
      <circle cx="12" cy="12" r="9" />
    </template>
    <template v-else-if="name === 'check'">
      <path d="M20 6 9 17l-5-5" />
    </template>
    <template v-else-if="name === 'alert'">
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
    </template>
    <template v-else-if="name === 'activity'">
      <path d="M22 12h-4l-3 9L9 3l-3 9H2" />
    </template>
    <template v-else-if="name === 'menu'">
      <path d="M4 6h16" /><path d="M4 12h16" /><path d="M4 18h16" />
    </template>
    <template v-else-if="name === 'panels'">
      <rect x="3" y="3" width="7" height="18" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </template>
    <template v-else-if="name === 'x'">
      <path d="M18 6 6 18" /><path d="M6 6l12 12" />
    </template>
    <template v-else-if="name === 'info'">
      <circle cx="12" cy="12" r="10" />
      <path d="M12 16v-4" /><path d="M12 8h.01" />
    </template>
    <template v-else-if="name === 'loader'">
      <path d="M12 2v4" /><path d="M12 18v4" /><path d="M4.93 4.93l2.83 2.83" />
      <path d="M16.24 16.24l2.83 2.83" /><path d="M2 12h4" /><path d="M18 12h4" />
      <path d="M4.93 19.07l2.83-2.83" /><path d="M16.24 7.76l2.83-2.83" />
    </template>
    <template v-else-if="name === 'list'">
      <path d="M8 6h13" /><path d="M8 12h13" /><path d="M8 18h13" />
      <path d="M3 6h.01" /><path d="M3 12h.01" /><path d="M3 18h.01" />
    </template>
    <template v-else-if="name === 'clock'">
      <circle cx="12" cy="12" r="10" />
      <path d="M12 6v6l4 2" />
    </template>
    <template v-else-if="name === 'play'">
      <path d="M5 3l14 9-14 9V3z" />
    </template>
    <template v-else-if="name === 'message'">
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
    </template>
    <template v-else-if="name === 'gitBranch'">
      <path d="M6 3v12" />
      <circle cx="18" cy="6" r="3" />
      <circle cx="6" cy="18" r="3" />
      <path d="M18 9a9 9 0 0 1-9 9" />
    </template>
    <template v-else-if="name === 'shield'">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
    </template>
    <template v-else-if="name === 'filter'">
      <path d="M22 3H2l8 9.46V19l4 2v-8.54L22 3z" />
    </template>
    <template v-else-if="name === 'help'">
      <circle cx="12" cy="12" r="10" />
      <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
      <path d="M12 17h.01" />
    </template>
    <template v-else-if="name === 'chevronDown'">
      <path d="M6 9l6 6 6-6" />
    </template>
    <template v-else-if="name === 'chevronRight'">
      <path d="M9 18l6-6-6-6" />
    </template>
  </Icon>
</template>
