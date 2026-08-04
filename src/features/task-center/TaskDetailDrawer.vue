<script setup lang="ts">
import { computed } from "vue";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import Drawer from "../../shared/ui/Drawer.vue";
import Skeleton from "../../shared/ui/Skeleton.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";
import { formatTimestamp } from "./format";
import { capabilitiesForStatus, presentTaskStatus } from "./status-map";
import type { TaskDetailViewModel } from "./types";

const props = defineProps<{
  open: boolean;
  detail: TaskDetailViewModel | null;
  loading?: boolean;
  cancelPending?: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  cancel: [taskId: string];
  recover: [taskId: string];
}>();

const task = computed(() => props.detail?.task ?? null);
const presentation = computed(() =>
  task.value ? presentTaskStatus(task.value.status) : null,
);
const caps = computed(() =>
  task.value ? capabilitiesForStatus(task.value.status) : null,
);
const title = computed(
  () => props.detail?.openTitle || task.value?.title || "任务详情",
);
</script>

<template>
  <Drawer
    :model-value="open"
    :title="title"
    @update:model-value="emit('update:open', $event)"
  >
    <div v-if="loading && !detail" class="detail-loading" data-testid="detail-loading">
      <Skeleton height="20px" />
      <Skeleton height="14px" width="70%" />
      <Skeleton height="14px" width="50%" />
    </div>

    <div v-else-if="task && presentation && caps" class="detail" data-testid="task-detail">
      <section class="detail-section">
        <h3>状态</h3>
        <StatusIcon :status="presentation.icon" :label="presentation.label" />
        <Badge v-if="task.status === 'waiting_permission'" tone="warning">待处理</Badge>
      </section>

      <section class="detail-section">
        <h3>时间</h3>
        <dl class="detail-dl">
          <div>
            <dt>创建</dt>
            <dd>{{ formatTimestamp(task.createdAt) }}</dd>
          </div>
          <div>
            <dt>更新</dt>
            <dd>{{ formatTimestamp(task.updatedAt) }}</dd>
          </div>
        </dl>
      </section>

      <section class="detail-section">
        <h3>项目</h3>
        <p>{{ task.projectLabel }}</p>
      </section>

      <section class="detail-section">
        <h3>会话 attempt</h3>
        <p v-if="task.sessionId">
          会话 {{ task.sessionId }}
          <span v-if="task.sessionState">（{{ task.sessionState }}）</span>
        </p>
        <p v-else class="muted">暂无绑定会话（占位）</p>
      </section>

      <section class="detail-section">
        <h3>Worktree 摘要</h3>
        <template v-if="task.worktreeDisplayPath || task.branch">
          <dl class="detail-dl">
            <div v-if="task.branch">
              <dt>分支</dt>
              <dd>{{ task.branch }}</dd>
            </div>
            <div v-if="task.baseBranch">
              <dt>基础分支</dt>
              <dd>{{ task.baseBranch }}</dd>
            </div>
            <div v-if="task.worktreeDisplayPath">
              <dt>路径</dt>
              <dd>{{ task.worktreeDisplayPath }}</dd>
            </div>
            <div v-if="task.worktreeState">
              <dt>状态</dt>
              <dd>{{ task.worktreeState }}</dd>
            </div>
          </dl>
        </template>
        <p v-else class="muted">Worktree 信息占位 — 后续由工作区模块填充</p>
      </section>

      <section v-if="task.interruptReason" class="detail-section">
        <h3>中断原因</h3>
        <p class="error-text">{{ task.interruptReason }}</p>
      </section>

      <section v-if="detail?.compatibilityError" class="detail-section" role="alert">
        <h3>兼容性提示</h3>
        <p class="error-text">{{ detail.compatibilityError }}</p>
      </section>

      <section v-if="task.localError" class="detail-section" role="alert">
        <h3>任务错误</h3>
        <p class="error-text">{{ task.localError }}</p>
      </section>

      <section class="detail-actions">
        <!-- Safe actions first so focus trap lands on close (Drawer) or open, not danger. -->
        <Button
          v-if="caps.canRecover"
          variant="primary"
          data-testid="detail-recover"
          @click="emit('recover', task.id)"
        >
          恢复任务
        </Button>
        <Button
          v-if="caps.canCancel"
          variant="danger"
          :state="cancelPending ? 'loading' : 'default'"
          :disabled="cancelPending"
          data-testid="detail-cancel"
          @click="emit('cancel', task.id)"
        >
          取消任务
        </Button>
      </section>
    </div>

    <p v-else class="muted">未选择任务</p>
  </Drawer>
</template>

<style scoped>
.detail {
  display: grid;
  gap: var(--space-4);
}
.detail-loading {
  display: grid;
  gap: var(--space-3);
}
.detail-section h3 {
  margin: 0 0 var(--space-2);
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.detail-section p {
  margin: 0;
  color: var(--ctp-text);
  word-break: break-word;
}
.detail-dl {
  margin: 0;
  display: grid;
  gap: var(--space-2);
}
.detail-dl div {
  display: grid;
  gap: 2px;
}
.detail-dl dt {
  color: var(--ctp-overlay0);
  font-size: var(--font-small);
}
.detail-dl dd {
  margin: 0;
  color: var(--ctp-text);
  word-break: break-word;
}
.muted {
  color: var(--ctp-subtext0);
}
.error-text {
  color: var(--ctp-peach);
}
.detail-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding-top: var(--space-2);
  border-top: 1px solid var(--ctp-surface0);
}
</style>
