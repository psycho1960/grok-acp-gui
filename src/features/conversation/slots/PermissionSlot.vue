<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import Badge from "../../../shared/ui/Badge.vue";
import Button from "../../../shared/ui/Button.vue";
import type { PermissionSlotView } from "../types";

const props = defineProps<{ slotData: PermissionSlotView }>();
const emit = defineEmits<{ resolve: [optionId: string] }>();
const root = ref<HTMLElement | null>(null);
const nowEpochSeconds = ref(Math.floor(Date.now() / 1000));
let expiryTimer: ReturnType<typeof setInterval> | null = null;
const effectiveExpired = computed(
  () => props.slotData.expired || nowEpochSeconds.value > props.slotData.expiresAtEpochSeconds,
);
const inactive = computed(
  () =>
    effectiveExpired.value ||
    props.slotData.decisionState === "submitting" ||
    props.slotData.decisionState === "resolved",
);
const orderedOptions = computed(() =>
  [...props.slotData.options].sort(
    (left, right) => Number(!isReject(left.kind)) - Number(!isReject(right.kind)),
  ),
);
function isReject(kind?: string): boolean {
  return kind === "reject" || kind === "deny" || kind === "reject_once" || kind === "reject_always" || kind === "cancel";
}
function isAllow(kind?: string): boolean {
  return kind === "allow_once" || kind === "approve_once" || kind === "allow_always" || kind === "allow_scope" || kind === "approve_scope";
}
function isKnown(kind?: string): boolean {
  return isReject(kind) || isAllow(kind);
}
/** Operations the backend cannot classify are never authorizable. */
function optionBlocked(opt: { kind?: string }): boolean {
  return props.slotData.operation.category === "unknown" && isAllow(opt.kind);
}
function choose(optionId: string): void {
  if (!inactive.value) emit("resolve", optionId);
}
onMounted(() => {
  expiryTimer = setInterval(() => {
    nowEpochSeconds.value = Math.floor(Date.now() / 1000);
  }, 1_000);
  void nextTick(() => root.value?.querySelector<HTMLElement>("[data-safe-default='true']")?.focus());
});
onBeforeUnmount(() => {
  if (expiryTimer) clearInterval(expiryTimer);
});
</script>

<template>
  <section
    ref="root"
    class="perm-slot surface-card"
    data-testid="permission-slot"
    aria-label="权限请求插槽"
  >
    <header>
      <Badge tone="warning">权限</Badge>
      <span class="title">{{ slotData.toolCall.title ?? "工具请求" }}</span>
      <Badge v-if="effectiveExpired" tone="danger">请求已失效</Badge>
    </header>
    <p class="meta">
      {{ slotData.operation.category }} · {{ slotData.toolCall.kind ?? "tool" }}
      <template v-if="slotData.toolCall.locations?.length">
        · {{ slotData.toolCall.locations.join(", ") }}
      </template>
    </p>
    <dl class="operation">
      <template v-if="slotData.operation.executable">
        <dt>命令</dt>
        <dd><code>{{ slotData.operation.executable }}</code></dd>
      </template>
      <template v-if="slotData.operation.args?.length">
        <dt>参数</dt>
        <dd><code>{{ slotData.operation.args.join(" ") }}</code></dd>
      </template>
      <template v-if="slotData.operation.cwd">
        <dt>工作目录</dt>
        <dd><code>{{ slotData.operation.cwd }}</code></dd>
      </template>
      <template v-if="slotData.operation.readPaths?.length || slotData.operation.writePaths?.length">
        <dt>影响路径</dt>
        <dd>{{ [...(slotData.operation.readPaths ?? []), ...(slotData.operation.writePaths ?? [])].join(", ") }}</dd>
      </template>
    </dl>
    <p class="risk">{{ slotData.operation.risk }}</p>
    <p v-if="slotData.errorMessage" class="error" role="alert">{{ slotData.errorMessage }}</p>
    <p v-if="slotData.decisionState === 'resolved'" class="resolved" role="status">决定已提交</p>
    <ul class="options">
      <li v-for="opt in orderedOptions" :key="opt.optionId">
        <Button
          :variant="isReject(opt.kind) ? 'secondary' : 'primary'"
          :state="slotData.decisionState === 'submitting' && slotData.selectedOptionId === opt.optionId ? 'loading' : 'default'"
          :disabled="inactive || !isKnown(opt.kind) || optionBlocked(opt)"
          :data-safe-default="isReject(opt.kind) || undefined"
          @click="choose(opt.optionId)"
        >
          {{ opt.name }}
        </Button>
        <span v-if="!isKnown(opt.kind)" class="unknown">语义未知，已阻止</span>
        <span v-else-if="optionBlocked(opt)" class="unknown">无法安全分类，已阻止</span>
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
.operation { display: grid; grid-template-columns: max-content 1fr; gap: var(--space-1) var(--space-2); margin: 0; font-size: var(--font-small); }
.operation dt { color: var(--ctp-subtext0); }
.operation dd { margin: 0; overflow-wrap: anywhere; }
.risk { margin: 0; padding: var(--space-2); border-left: 3px solid var(--ctp-yellow); background: var(--ctp-surface0); }
.error { margin: 0; color: var(--ctp-red); }
.resolved { margin: 0; color: var(--ctp-green); }
.unknown { color: var(--ctp-overlay1); font-size: var(--font-small); }
.options {
  margin: 0;
  padding: 0;
  list-style: none;
  display: grid;
  gap: var(--space-1);
}
.options li {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--font-small);
}
code {
  color: var(--ctp-overlay1);
  font-family: var(--font-mono);
}
</style>
