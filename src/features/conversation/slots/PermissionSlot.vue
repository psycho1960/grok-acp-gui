<script setup lang="ts">
import Badge from "../../../shared/ui/Badge.vue";
import type { PermissionSlotView } from "../types";

defineProps<{ slotData: PermissionSlotView }>();
</script>

<template>
  <section
    class="perm-slot surface-card"
    data-testid="permission-slot"
    aria-label="权限请求插槽"
  >
    <header>
      <Badge tone="warning">权限</Badge>
      <span class="title">{{ slotData.toolCall.title ?? "工具请求" }}</span>
      <Badge v-if="slotData.expired" tone="danger">请求已失效</Badge>
    </header>
    <p class="meta">
      {{ slotData.toolCall.kind ?? "tool" }}
      <template v-if="slotData.toolCall.locations?.length">
        · {{ slotData.toolCall.locations.join(", ") }}
      </template>
    </p>
    <p class="hint">审批 UI 由 GAG-009 承接；此处保留稳定插槽与 option ID。</p>
    <ul class="options">
      <li v-for="opt in slotData.options" :key="opt.optionId">
        <span>{{ opt.name }}</span>
        <code>{{ opt.optionId }}</code>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.perm-slot {
  padding: var(--space-3);
  display: grid;
  gap: var(--space-2);
  border-color: var(--ctp-yellow);
}
header {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
}
.title {
  font-weight: 600;
}
.meta,
.hint {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.options {
  margin: 0;
  padding: 0;
  list-style: none;
  display: grid;
  gap: var(--space-1);
}
.options li {
  display: flex;
  justify-content: space-between;
  gap: var(--space-2);
  font-size: var(--font-small);
}
code {
  color: var(--ctp-overlay1);
  font-family: var(--font-mono);
}
</style>
