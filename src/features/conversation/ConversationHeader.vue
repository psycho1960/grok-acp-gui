<script setup lang="ts">
import { computed } from "vue";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import Select from "../../shared/ui/Select.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";
import type { ModelInfo, ReasoningEffort } from "../../bridge/types";
import type { ConversationRunStatus } from "./types";

const props = defineProps<{
  title: string;
  status: ConversationRunStatus;
  attempt?: number;
  canCancel?: boolean;
  needsRefresh?: boolean;
  models?: ModelInfo[];
  selectedModel?: string | null;
  selectedReasoning?: ReasoningEffort | null;
  settingsDisabled?: boolean;
}>();

const emit = defineEmits<{
  cancel: [];
  refresh: [];
  resume: [];
  "update:model": [model: string | null];
  "update:reasoning": [reasoning: ReasoningEffort];
}>();

function statusIcon(
  s: ConversationRunStatus,
): "running" | "waiting" | "success" | "error" | "interrupted" {
  switch (s) {
    case "running":
    case "cancelling":
      return "running";
    case "waiting_permission":
    case "waiting_plan":
      return "waiting";
    case "error":
    case "offline":
    case "disconnected":
      return "error";
    default:
      return "success";
  }
}

function statusLabel(s: ConversationRunStatus): string {
  const map: Record<ConversationRunStatus, string> = {
    idle: "空闲",
    running: "运行中",
    waiting_permission: "等待权限",
    waiting_plan: "等待计划审批",
    cancelling: "停止中",
    error: "错误",
    disconnected: "已断开",
    offline: "离线",
  };
  return map[s];
}

const tone = () => {
  if (props.status === "error" || props.status === "offline") return "danger" as const;
  if (props.status === "running") return "info" as const;
  if (props.status === "waiting_permission" || props.status === "waiting_plan")
    return "warning" as const;
  return "neutral" as const;
};

const modelOptions = computed(() => [
  { value: "", label: "使用运行时默认模型" },
  ...(props.models ?? [])
    .filter((model) => model.modelId.trim().length > 0)
    .map((model) => ({
      value: model.modelId,
      label: model.name || model.modelId,
    })),
]);

const reasoningOptions = computed(() => [
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "max", label: "最高" },
]);

function onModelChange(value: string): void {
  emit("update:model", value === "" ? null : value);
}

function onReasoningChange(value: string): void {
  if (value === "low" || value === "medium" || value === "high" || value === "max") {
    emit("update:reasoning", value);
  }
}
</script>

<template>
  <header class="conv-header" data-testid="conversation-header">
    <div class="left">
      <h1 class="title">{{ title }}</h1>
      <StatusIcon :status="statusIcon(status)" :label="statusLabel(status)" />
      <Badge :tone="tone()">{{ statusLabel(status) }}</Badge>
      <Badge v-if="attempt" tone="neutral">第 {{ attempt }} 次尝试</Badge>
    </div>
    <div class="right">
      <div class="settings" aria-label="对话设置">
        <Select
          class="settings-select"
          data-testid="conversation-model-select"
          label="模型"
          :model-value="selectedModel ?? ''"
          :options="modelOptions"
          :disabled="settingsDisabled"
          @update:model-value="onModelChange"
        />
        <Select
          class="settings-select"
          data-testid="conversation-reasoning-select"
          label="推理强度"
          :model-value="selectedReasoning ?? 'medium'"
          :options="reasoningOptions"
          :disabled="settingsDisabled"
          @update:model-value="onReasoningChange"
        />
      </div>
      <Button
        v-if="status === 'error' || status === 'disconnected'"
        variant="primary"
        data-testid="resume-session"
        @click="emit('resume')"
      >
        恢复会话
      </Button>
      <Button
        v-if="needsRefresh"
        variant="secondary"
        data-testid="refresh-snapshot"
        @click="emit('refresh')"
      >
        刷新快照
      </Button>
      <Button
        v-if="canCancel"
        variant="danger"
        data-testid="header-stop"
        @click="emit('cancel')"
      >
        停止
      </Button>
    </div>
  </header>
</template>

<style scoped>
.conv-header {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3);
  border-bottom: 1px solid var(--ctp-surface0);
  background: var(--ctp-mantle);
}
.left,
.right {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
}
.title {
  margin: 0;
  font-size: 16px;
  color: var(--ctp-text);
}
.settings {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: end;
}
.settings-select {
  min-width: 132px;
}
</style>
