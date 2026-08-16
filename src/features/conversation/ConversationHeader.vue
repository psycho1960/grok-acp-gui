<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import IconButton from "../../shared/ui/IconButton.vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";
import Tooltip from "../../shared/ui/Tooltip.vue";
import { modeHelpFor } from "../../shared/ui/mode-help";
import type { ModeInfo } from "../../bridge/types";
import {
  WORKSPACE_STRATEGY_OPTIONS,
  workspaceStrategyForMode,
  type WorkspaceStrategy,
} from "./mode-workspace";
import type { ConversationRunStatus } from "./types";
import HeaderSelect from "./HeaderSelect.vue";

export interface ConversationTurn {
  id: string;
  seq: number;
  firstLine: string;
  timestamp: string;
}

const props = defineProps<{
  title: string;
  status: ConversationRunStatus;
  attempt?: number;
  needsRefresh?: boolean;
  modes?: ModeInfo[];
  selectedMode?: string | null;
  selectedWorkspaceStrategy?: WorkspaceStrategy | null;
  settingsDisabled?: boolean;
  turns?: ConversationTurn[];
}>();

const emit = defineEmits<{
  back: [];
  refresh: [];
  resume: [];
  "update:mode": [mode: string | null, strategy: WorkspaceStrategy | null];
  "update:workspaceStrategy": [strategy: WorkspaceStrategy];
  "jump-turn": [id: string];
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

/** 中文模式标签：capability 名称优先，应用自有词汇兜底。 */
const MODE_LABEL_FALLBACK: Record<string, string> = {
  agent: "智能体",
  plan: "计划",
  ask: "问答",
};

type ModeOption = { value: string; label: string };

const PRODUCT_MODES: readonly ModeOption[] = [
  { value: "agent", label: "智能体" },
  { value: "plan", label: "计划" },
  { value: "ask", label: "问答" },
];

const modeOptions = computed(() => {
  const available = PRODUCT_MODES.map((mode) => ({ ...mode }));
  const additional = (props.modes ?? [])
    .filter((mode) => mode.id.trim().length > 0)
    .filter((mode) => !available.some((option) => option.value === mode.id))
    .map((mode) => ({
      value: mode.id,
      label: MODE_LABEL_FALLBACK[mode.id] || mode.name || mode.id,
    }));
  available.push(...additional);
  const selected = props.selectedMode?.trim() ?? "";
  if (selected && !available.some((mode) => mode.value === selected)) {
    available.push({
      value: selected,
      label: MODE_LABEL_FALLBACK[selected] || selected,
    });
  }
  return [{ value: "", label: "使用会话默认模式" }, ...available];
});

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

const modeLabel = computed(() => {
  const selected = props.selectedMode ?? "";
  return (
    modeOptions.value.find((option) => option.value === selected)?.label ??
    "使用会话默认模式"
  );
});

const workspaceLabel = computed(() => {
  const selected = props.selectedWorkspaceStrategy ?? "";
  if (selected === "") return "使用创建时的策略";
  return (
    WORKSPACE_STRATEGY_OPTIONS.find((option) => option.value === selected)
      ?.label ?? selected
  );
});

const turnListOpen = ref(false);
const turnHistoryRoot = ref<HTMLElement | null>(null);

const turns = computed(() => props.turns ?? []);

function formatRelativeTime(iso: string): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const minutes = Math.floor(Math.max(0, Date.now() - then) / 60_000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
}

function toggleTurnList(): void {
  if (turns.value.length === 0) return;
  turnListOpen.value = !turnListOpen.value;
}

function closeTurnList(): void {
  turnListOpen.value = false;
}

function onJumpTurn(id: string): void {
  turnListOpen.value = false;
  emit("jump-turn", id);
}

function onDocumentPointerDown(event: PointerEvent): void {
  const target = event.target;
  if (!(target instanceof Node)) return;
  if (turnHistoryRoot.value && !turnHistoryRoot.value.contains(target))
    closeTurnList();
}

function onDocumentKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") closeTurnList();
}

onMounted(() => {
  window.addEventListener("pointerdown", onDocumentPointerDown);
  window.addEventListener("keydown", onDocumentKeydown);
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", onDocumentPointerDown);
  window.removeEventListener("keydown", onDocumentKeydown);
});
</script>

<template>
  <header class="conv-header" data-testid="conversation-header">
    <div class="left">
      <IconButton
        label="返回任务中心"
        data-testid="conversation-back"
        @click="emit('back')"
      >
        <NamedIcon name="chevronLeft" :size="16" />
      </IconButton>
      <h1 class="title">{{ title }}</h1>
      <StatusIcon
        data-testid="conversation-status"
        :status="statusIcon(status)"
        :label="statusLabel(status)"
      />
      <Badge v-if="attempt && attempt > 1" tone="neutral">
        第 {{ attempt }} 次尝试
      </Badge>
      <div
        ref="turnHistoryRoot"
        class="turn-history"
        data-testid="turn-history-menu"
      >
        <IconButton
          label="历史轮次"
          data-testid="turn-history"
          :disabled="turns.length === 0"
          @click="toggleTurnList"
        >
          <NamedIcon name="clock" :size="16" />
        </IconButton>
        <ul
          v-if="turnListOpen"
          class="turn-list"
          data-testid="turn-list"
          role="listbox"
          aria-label="历史轮次"
        >
          <li v-for="turn in turns" :key="turn.id">
            <button
              type="button"
              class="turn-row"
              data-testid="turn-row"
              :data-seq="turn.seq"
              @click="onJumpTurn(turn.id)"
            >
              <span class="turn-line">{{ turn.firstLine }}</span>
              <time class="turn-time" :datetime="turn.timestamp">{{
                formatRelativeTime(turn.timestamp)
              }}</time>
            </button>
          </li>
        </ul>
      </div>
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
            <span v-if="settingsLocked" class="locked-label">{{
              modeLabel
            }}</span>
            <HeaderSelect
              class="settings-select"
              :class="{ 'is-visually-hidden': settingsLocked }"
              label="模式"
              :model-value="selectedMode ?? ''"
              :options="modeOptions"
              :disabled="settingsLocked"
              @update:model-value="onModeChange"
            >
              <template #indicator>
                <NamedIcon
                  v-if="!settingsLocked"
                  name="chevronDown"
                  :size="12"
                  data-testid="mode-chevron"
                />
              </template>
            </HeaderSelect>
          </div>
          <Tooltip :text="modeHelpFor(selectedMode)" placement="bottom">
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
          <span v-if="settingsLocked" class="locked-label">{{
            workspaceLabel
          }}</span>
          <HeaderSelect
            class="settings-select"
            :class="{ 'is-visually-hidden': settingsLocked }"
            label="工作区策略"
            :model-value="selectedWorkspaceStrategy ?? ''"
            :options="WORKSPACE_STRATEGY_OPTIONS"
            :disabled="settingsLocked"
            @update:model-value="onWorkspaceStrategyChange"
          >
            <template #indicator>
              <NamedIcon
                v-if="!settingsLocked"
                name="chevronDown"
                :size="12"
                data-testid="workspace-chevron"
              />
            </template>
          </HeaderSelect>
        </div>
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
.session-badge .is-visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
.turn-history {
  position: relative;
}
.turn-list {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 4;
  display: grid;
  gap: 2px;
  min-width: 220px;
  max-width: min(360px, 70vw);
  max-height: 280px;
  margin: 0;
  padding: var(--space-1);
  overflow: auto;
  list-style: none;
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  box-shadow: var(--shadow-md);
}
.turn-row {
  display: flex;
  gap: var(--space-2);
  align-items: baseline;
  justify-content: space-between;
  width: 100%;
  padding: 6px 8px;
  color: var(--ctp-text);
  background: transparent;
  border: 0;
  border-radius: var(--radius-control);
  cursor: pointer;
  text-align: left;
}
.turn-row:hover,
.turn-row:focus-visible {
  background: var(--overlay-hover);
}
.turn-line {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.turn-time {
  flex-shrink: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
</style>
