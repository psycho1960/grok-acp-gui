<script setup lang="ts">
import { computed } from "vue";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import IconButton from "../../shared/ui/IconButton.vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import Select from "../../shared/ui/Select.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";
import Tooltip from "../../shared/ui/Tooltip.vue";
import { modeHelpFor } from "../../shared/ui/mode-help";
import type { ModeInfo, ModelInfo, ReasoningEffort } from "../../bridge/types";
import {
  WORKSPACE_STRATEGY_OPTIONS,
  workspaceStrategyForMode,
  type WorkspaceStrategy,
} from "./mode-workspace";
import type { ConversationRunStatus } from "./types";

const props = defineProps<{
  title: string;
  status: ConversationRunStatus;
  attempt?: number;
  canCancel?: boolean;
  needsRefresh?: boolean;
  modes?: ModeInfo[];
  models?: ModelInfo[];
  selectedMode?: string | null;
  selectedWorkspaceStrategy?: WorkspaceStrategy | null;
  selectedModel?: string | null;
  selectedReasoning?: ReasoningEffort | null;
  settingsDisabled?: boolean;
}>();

const emit = defineEmits<{
  back: [];
  cancel: [];
  refresh: [];
  resume: [];
  "update:mode": [mode: string | null, strategy: WorkspaceStrategy | null];
  "update:workspaceStrategy": [strategy: WorkspaceStrategy];
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

const settingsLocked = computed(() => Boolean(props.settingsDisabled));

const lockReason = computed(() => {
  switch (props.status) {
    case "running":
      return "运行中无法切换会话身份";
    case "cancelling":
      return "停止中无法切换会话身份";
    case "waiting_permission":
    case "waiting_plan":
      return "等待审批时无法切换会话身份";
    default:
      return "发送或保存中无法切换会话身份";
  }
});

const modelOptions = computed(() => [
  { value: "", label: "使用运行时默认模型" },
  ...(props.models ?? [])
    .filter((model) => model.modelId.trim().length > 0)
    .map((model) => ({
      value: model.modelId,
      label: model.name || model.modelId,
    })),
]);

/** 中文模式标签：capability 名称优先，应用自有词汇兜底。 */
const MODE_LABEL_FALLBACK: Record<string, string> = {
  agent: "智能体",
  plan: "计划",
  ask: "问答",
};

const modeOptions = computed(() => [
  { value: "", label: "使用会话默认模式" },
  ...(props.modes ?? [])
    .filter((mode) => mode.id.trim().length > 0)
    .map((mode) => ({
      value: mode.id,
      label: mode.name || MODE_LABEL_FALLBACK[mode.id] || mode.id,
    })),
]);

const reasoningOptions = computed(() => [
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "max", label: "最高" },
]);

function onModeChange(value: string): void {
  const mode = value === "" ? null : value;
  // One event becomes one atomic session.configure call in the store.
  emit("update:mode", mode, workspaceStrategyForMode(mode));
}

function onWorkspaceStrategyChange(value: string): void {
  if (value === "worktree" || value === "readonly" || value === "direct") {
    emit("update:workspaceStrategy", value);
  }
}

function onModelChange(value: string): void {
  emit("update:model", value === "" ? null : value);
}

function onReasoningChange(value: string): void {
  if (value === "low" || value === "medium" || value === "high" || value === "max") {
    emit("update:reasoning", value);
  }
}

const modeLabel = computed(() => {
  const selected = props.selectedMode ?? "";
  return modeOptions.value.find((option) => option.value === selected)?.label ?? "使用会话默认模式";
});

const workspaceLabel = computed(() => {
  const selected = props.selectedWorkspaceStrategy ?? "";
  if (selected === "") return "使用创建时的策略";
  return WORKSPACE_STRATEGY_OPTIONS.find((option) => option.value === selected)?.label ?? selected;
});
</script>

<template>
  <header class="conv-header" data-testid="conversation-header">
    <div class="left">
      <IconButton label="返回任务中心" data-testid="conversation-back" @click="emit('back')">
        <NamedIcon name="chevronLeft" :size="16" />
      </IconButton>
      <h1 class="title">{{ title }}</h1>
      <StatusIcon
        data-testid="conversation-status"
        :status="statusIcon(status)"
        :label="statusLabel(status)"
      />
      <Badge v-if="attempt && attempt > 1" tone="neutral">第 {{ attempt }} 次尝试</Badge>
    </div>
    <div class="right">
      <div class="settings" aria-label="对话设置">
        <div class="mode-field">
          <div
            class="session-badge"
            :class="{ locked: settingsLocked }"
            data-testid="conversation-mode-select"
            :title="settingsLocked ? lockReason : undefined"
          >
            <span v-if="settingsLocked" class="locked-label">{{ modeLabel }}</span>
            <Select
              class="settings-select"
              :class="{ 'is-visually-hidden': settingsLocked }"
              label="模式"
              :model-value="selectedMode ?? ''"
              :options="modeOptions"
              :disabled="settingsLocked"
              @update:model-value="onModeChange"
            />
            <NamedIcon
              v-if="!settingsLocked"
              name="chevronDown"
              :size="12"
              data-testid="mode-chevron"
            />
          </div>
          <Tooltip :text="modeHelpFor(selectedMode)">
            <IconButton label="模式说明" data-testid="conversation-mode-help">
              <NamedIcon name="help" :size="14" />
            </IconButton>
          </Tooltip>
        </div>
        <div
          class="session-badge"
          :class="{ locked: settingsLocked }"
          data-testid="conversation-workspace-select"
          :title="settingsLocked ? lockReason : undefined"
        >
          <span v-if="settingsLocked" class="locked-label">{{ workspaceLabel }}</span>
          <Select
            class="settings-select"
            :class="{ 'is-visually-hidden': settingsLocked }"
            label="工作区策略"
            :model-value="selectedWorkspaceStrategy ?? ''"
            :options="[{ value: '', label: '使用创建时的策略' }, ...WORKSPACE_STRATEGY_OPTIONS]"
            :disabled="settingsLocked"
            @update:model-value="onWorkspaceStrategyChange"
          />
          <NamedIcon
            v-if="!settingsLocked"
            name="chevronDown"
            :size="12"
            data-testid="workspace-chevron"
          />
        </div>
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
  font-size: var(--heading-panel);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
  color: var(--ctp-text);
}
.settings {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: end;
}
.mode-field {
  display: flex;
  gap: var(--space-1);
  align-items: end;
}
.settings-select {
  min-width: 132px;
}
.session-badge {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  min-height: var(--control-min-size);
  padding: 0 var(--space-2);
  color: var(--ctp-text);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: 999px;
}
.session-badge.locked {
  color: var(--ctp-subtext0);
  cursor: default;
}
.session-badge :deep(.field > span) {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
.session-badge :deep(select) {
  min-height: 28px;
  padding: 0;
  color: inherit;
  background: transparent;
  border: 0;
  appearance: none;
}
.session-badge .is-visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
</style>
