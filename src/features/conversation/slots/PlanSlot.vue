<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import Badge from "../../../shared/ui/Badge.vue";
import Button from "../../../shared/ui/Button.vue";
import type { PlanSlotView } from "../types";

const props = defineProps<{ slotData: PlanSlotView }>();
const emit = defineEmits<{ resolve: [optionId: string] }>();
const root = ref<HTMLElement | null>(null);
const inactive = computed(() => props.slotData.approvalInvalidated || props.slotData.decisionState === "submitting" || props.slotData.decisionState === "resolved");
function isSafe(kind?: string): boolean {
  return kind === "cancel" || kind === "reject" || kind === "reject_once" || kind === "reject_always" || kind === "continue" || kind === "continue_planning" || kind === "request_revision" || kind === "revision_requested";
}
function isKnown(kind?: string): boolean {
  return isSafe(kind) || kind === "approve" || kind === "allow_once";
}
onMounted(() => {
  void nextTick(() => root.value?.querySelector<HTMLElement>("[data-safe-default='true']")?.focus());
});
</script>

<template>
  <section ref="root" class="plan-slot surface-card" data-testid="plan-slot" aria-label="计划审批">
    <header>
      <Badge tone="info">计划</Badge>
      <span class="status">{{ slotData.status }}</span>
      <Badge tone="neutral">v{{ slotData.version }}</Badge>
      <Badge v-if="slotData.approvalInvalidated" tone="danger">批准已失效</Badge>
    </header>
    <p class="banner">规划阶段：写入与非只读命令已阻止</p>
    <p class="detail">{{ slotData.detailSummary }}</p>
    <ol v-if="slotData.steps.length" class="steps">
      <li v-for="step in slotData.steps" :key="step">{{ step }}</li>
    </ol>
    <p v-if="slotData.errorMessage" class="error" role="alert">{{ slotData.errorMessage }}</p>
    <div class="actions" data-testid="plan-actions" data-align="end">
      <Button
        v-for="option in slotData.options"
        :key="option.optionId"
        :variant="isSafe(option.kind) ? 'secondary' : 'primary'"
        :state="slotData.decisionState === 'submitting' && slotData.selectedOptionId === option.optionId ? 'loading' : 'default'"
        :disabled="inactive || !isKnown(option.kind)"
        :data-safe-default="isSafe(option.kind) || undefined"
        data-label-align="center"
        @click="emit('resolve', option.optionId)"
      >
        {{ option.name }}
      </Button>
    </div>
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
.steps { margin: 0; padding-left: var(--space-5); display: grid; gap: var(--space-1); }
.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--space-2); }
.error { margin: 0; color: var(--ctp-red); }
</style>
