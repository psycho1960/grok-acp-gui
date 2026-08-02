<script setup lang="ts">withDefaults(defineProps<{ width?: string; height?: string }>(), { width: "100%", height: "16px" });</script>
<template><span class="skeleton" :style="{ width, height }" aria-label="加载中" role="status" /></template>
<style scoped>.skeleton { display:block; border-radius:var(--radius-control); background:linear-gradient(90deg, var(--ctp-surface0), var(--ctp-surface1), var(--ctp-surface0)); background-size:200% 100%; animation:shimmer 1.4s ease infinite; }@keyframes shimmer { to { background-position:-200% 0; } }</style>
