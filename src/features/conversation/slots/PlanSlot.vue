<script setup lang="ts">
import Badge from "../../../shared/ui/Badge.vue";
import type { PlanSlotView } from "../types";

defineProps<{ slotData: PlanSlotView }>();
</script>

<template>
  <section class="plan-slot surface-card" data-testid="plan-slot" aria-label="Plan 插槽">
    <header>
      <Badge tone="info">Plan</Badge>
      <span class="status">{{ slotData.status }}</span>
    </header>
    <p class="banner">规划阶段：写入与非只读命令已阻止</p>
    <p class="detail">{{ slotData.detailSummary }}</p>
    <p class="hint">批准 / 继续规划 / 取消 由 GAG-009 实现。</p>
  </section>
</template>

<style scoped>
.plan-slot {
  padding: var(--space-3);
  display: grid;
  gap: var(--space-2);
  border-color: var(--ctp-blue);
}
header {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.status {
  font-weight: 600;
}
.banner {
  margin: 0;
  padding: var(--space-2);
  color: var(--ctp-yellow);
  background: var(--ctp-surface0);
  border-radius: var(--radius-control);
  font-size: var(--font-small);
}
.detail,
.hint {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
</style>
