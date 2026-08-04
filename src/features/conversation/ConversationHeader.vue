<script setup lang="ts">
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";
import type { ConversationRunStatus } from "./types";

const props = defineProps<{
  title: string;
  status: ConversationRunStatus;
  attempt?: number;
  canCancel?: boolean;
  needsRefresh?: boolean;
}>();

const emit = defineEmits<{ cancel: []; refresh: [] }>();

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
    waiting_plan: "等待 Plan",
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
</script>

<template>
  <header class="conv-header" data-testid="conversation-header">
    <div class="left">
      <h1 class="title">{{ title }}</h1>
      <StatusIcon :status="statusIcon(status)" :label="statusLabel(status)" />
      <Badge :tone="tone()">{{ statusLabel(status) }}</Badge>
      <Badge v-if="attempt" tone="neutral">attempt {{ attempt }}</Badge>
    </div>
    <div class="right">
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
</style>
