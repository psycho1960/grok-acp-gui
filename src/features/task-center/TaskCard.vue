<script setup lang="ts">
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";
import { formatDuration, formatRelative } from "./format";
import { capabilitiesForStatus, presentTaskStatus } from "./status-map";
import type { TaskViewModel } from "./types";

const props = defineProps<{
  task: TaskViewModel;
  selected?: boolean;
  cancelPending?: boolean;
}>();

const emit = defineEmits<{
  open: [taskId: string];
  cancel: [taskId: string];
  recover: [taskId: string];
}>();

const presentation = () => presentTaskStatus(props.task.status);
const caps = () => capabilitiesForStatus(props.task.status);

function onActivate(): void {
  emit("open", props.task.id);
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    onActivate();
  }
}
</script>

<template>
  <article
    class="task-card"
    :class="{ 'is-selected': selected, 'is-error': !!task.localError }"
    role="button"
    tabindex="0"
    :aria-selected="selected || undefined"
    :aria-label="`${task.title}，${presentation().label}`"
    :data-task-id="task.id"
    :data-status="task.status"
    @click="onActivate"
    @keydown="onKeydown"
  >
    <div class="task-card-main">
      <StatusIcon :status="presentation().icon" :label="presentation().label" />
      <div class="task-card-body">
        <div class="task-card-title-row">
          <h3 class="task-title">{{ task.title }}</h3>
          <Badge v-if="task.status === 'waiting_permission'" tone="warning">待处理</Badge>
          <Badge v-else-if="task.hasLiveSession" tone="info">会话中</Badge>
        </div>
        <p class="task-meta">
          <span>{{ task.projectLabel }}</span>
          <span aria-hidden="true">·</span>
          <span>{{ formatRelative(task.updatedAt) }}</span>
          <span aria-hidden="true">·</span>
          <span>持续 {{ formatDuration(task.createdAt, task.updatedAt) }}</span>
        </p>
        <p v-if="task.phase || task.latestActivity" class="task-activity">
          <span v-if="task.phase">{{ task.phase }}</span>
          <span v-if="task.phase && task.latestActivity" aria-hidden="true"> · </span>
          <span v-if="task.latestActivity">{{ task.latestActivity }}</span>
        </p>
        <p v-if="task.branch || task.worktreeDisplayPath" class="task-workspace">
          <span v-if="task.branch">{{ task.branch }}</span>
          <span v-if="task.branch && task.worktreeDisplayPath" aria-hidden="true"> · </span>
          <span v-if="task.worktreeDisplayPath">{{ task.worktreeDisplayPath }}</span>
        </p>
        <p v-if="task.localError" class="task-local-error" role="alert">{{ task.localError }}</p>
      </div>
    </div>
    <div class="task-card-actions" @click.stop>
      <Button
        v-if="caps().canCancel"
        variant="ghost"
        :state="cancelPending ? 'loading' : 'default'"
        :disabled="cancelPending"
        data-testid="task-cancel"
        @click="emit('cancel', task.id)"
      >
        取消
      </Button>
      <Button
        v-if="caps().canRecover"
        variant="secondary"
        data-testid="task-recover"
        @click="emit('recover', task.id)"
      >
        恢复
      </Button>
    </div>
  </article>
</template>

<style scoped>
.task-card {
  display: flex;
  gap: var(--space-3);
  align-items: flex-start;
  justify-content: space-between;
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  padding: var(--space-3);
  color: var(--ctp-text);
  text-align: left;
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface0);
  border-radius: var(--radius-card);
  cursor: pointer;
}
.task-card:hover {
  border-color: var(--ctp-surface1);
  background: var(--ctp-surface0);
}
.task-card:focus-visible {
  outline: 2px solid var(--ctp-mauve);
  outline-offset: 2px;
}
.task-card.is-selected {
  border-color: var(--ctp-mauve);
  box-shadow: inset 0 0 0 1px var(--ctp-mauve);
}
.task-card.is-error {
  border-color: var(--ctp-peach);
}
.task-card-main {
  display: flex;
  gap: var(--space-3);
  min-width: 0;
  flex: 1;
}
.task-card-body {
  min-width: 0;
  display: grid;
  gap: 2px;
}
.task-card-title-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
}
.task-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.3;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.task-meta,
.task-activity,
.task-workspace {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.task-local-error {
  margin: 0;
  color: var(--ctp-peach);
  font-size: var(--font-small);
}
.task-card-actions {
  display: flex;
  flex-shrink: 0;
  gap: var(--space-1);
}
</style>
