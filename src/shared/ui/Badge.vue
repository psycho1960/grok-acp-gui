<script setup lang="ts">
withDefaults(
  defineProps<{ tone?: "neutral" | "info" | "success" | "warning" | "danger" }>(),
  { tone: "neutral" },
);
</script>

<template>
  <span class="badge" :class="`tone-${tone}`"><slot /></span>
</template>

<style scoped>
.badge {
  display: inline-flex;
  align-items: center;
  min-height: 22px;
  padding: 2px 7px;
  border: 1px solid var(--ctp-surface1);
  border-radius: 999px;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  line-height: var(--leading-tight);
}
.tone-info {
  color: var(--ctp-text);
  background: var(--overlay-info);
  border-color: var(--border-tone-info);
}
.tone-success {
  color: var(--ctp-text);
  background: var(--overlay-success);
  border-color: var(--border-tone-success);
}
.tone-warning {
  color: var(--ctp-crust);
  background: var(--ctp-yellow);
  border-color: var(--ctp-yellow);
}
.tone-danger {
  color: var(--ctp-crust);
  background: var(--ctp-red);
  border-color: var(--ctp-red);
}
</style>
