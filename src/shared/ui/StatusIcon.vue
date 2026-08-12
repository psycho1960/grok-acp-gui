<script setup lang="ts">
import NamedIcon from "./NamedIcon.vue";
import type { IconName } from "./icons";

defineProps<{
  status: "running" | "waiting" | "success" | "error" | "interrupted";
  label: string;
}>();

const iconName: Record<"running" | "waiting" | "success" | "error" | "interrupted", IconName> = {
  running: "loader",
  waiting: "circle",
  success: "check",
  error: "alert",
  interrupted: "activity",
};
</script>

<template>
  <span class="status" :class="`is-${status}`" role="img" :aria-label="label">
    <span class="glyph" aria-hidden="true">
      <NamedIcon :name="iconName[status]" :size="14" :stroke-width="2.25" />
    </span>
    <span class="label">{{ label }}</span>
  </span>
</template>

<style scoped>
.status {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  font-size: var(--font-small);
}
.glyph {
  display: inline-grid;
  place-items: center;
  width: 16px;
  height: 16px;
}
.is-running {
  color: var(--ctp-blue);
}
.is-waiting {
  color: var(--ctp-yellow);
}
.is-success {
  color: var(--ctp-green);
}
.is-error {
  color: var(--ctp-red);
}
.is-interrupted {
  color: var(--ctp-peach);
}
.is-running .glyph {
  animation: spin 1s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(1turn);
  }
}
</style>
