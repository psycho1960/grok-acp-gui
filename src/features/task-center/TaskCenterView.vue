<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { DesktopBridge, TaskId, TaskStatus } from "../../bridge/types";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import Dialog from "../../shared/ui/Dialog.vue";
import EmptyState from "../../shared/ui/EmptyState.vue";
import ErrorState from "../../shared/ui/ErrorState.vue";
import Input from "../../shared/ui/Input.vue";
import Select from "../../shared/ui/Select.vue";
import Skeleton from "../../shared/ui/Skeleton.vue";
import {
  applyTaskCenterHash,
  parseTaskCenterHash,
} from "./hash-route";
import { buildGroupedListRows, type TaskListRow } from "./list-rows";
import TaskCard from "./TaskCard.vue";
import TaskDetailDrawer from "./TaskDetailDrawer.vue";
import { useTaskCenterStore } from "./task-center-store";
import type { TaskGroupId, UpdatedWithin } from "./types";
import { TASK_GROUP_LABELS } from "./types";
import VirtualList from "./VirtualList.vue";

const props = defineProps<{
  bridge: DesktopBridge;
  /** When true, sync selection with location.hash (#task-center[/id][?group=]). */
  syncHash?: boolean;
}>();

const store = useTaskCenterStore();
const confirmCancelOpen = ref(false);
const confirmCancelTaskId = ref<TaskId | null>(null);
const cancelFeedback = ref<string | null>(null);

/** Fixed row height for headers + cards (allows localError line without clipping). */
const ITEM_HEIGHT = 120;

const statusOptions = [
  { value: "all", label: "全部状态" },
  { value: "preparing", label: "准备中" },
  { value: "running", label: "运行中" },
  { value: "waiting_permission", label: "等待审批" },
  { value: "integrating", label: "集成中" },
  { value: "merged", label: "已合并" },
  { value: "archived", label: "已归档" },
  { value: "interrupted", label: "已中断" },
] as const;

const updatedOptions = [
  { value: "any", label: "任意时间" },
  { value: "1h", label: "最近 1 小时" },
  { value: "24h", label: "最近 24 小时" },
  { value: "7d", label: "最近 7 天" },
] as const;

const groupOptions = computed(() => [
  { value: "all", label: "全部分组" },
  ...Object.entries(TASK_GROUP_LABELS).map(([value, label]) => ({ value, label })),
]);

const projectFilterOptions = computed(() => [
  { value: "all", label: "全部项目" },
  ...store.projectOptions,
]);

const listRows = computed(() => buildGroupedListRows(store.groups));
const totalCount = computed(() => store.allTasks.length);
const visibleCount = computed(() => store.visibleTasks.length);
const drawerOpen = computed(() => store.selectedTaskId != null);

const isLoading = computed(() => store.loadState === "loading" || store.loadState === "idle");
const isEmpty = computed(
  () => store.loadState === "ready" && store.allTasks.length === 0,
);
const isFilteredEmpty = computed(
  () =>
    store.loadState === "ready" &&
    store.allTasks.length > 0 &&
    store.visibleTasks.length === 0,
);
const isError = computed(() => store.loadState === "error");
const isStale = computed(() => store.loadState === "stale");

function rowKey(item: TaskListRow): string {
  return item.key;
}

function onQuery(value: string): void {
  store.setFilters({ query: value });
}

function onStatus(value: string): void {
  store.setFilters({
    status: (value === "all" ? "all" : value) as TaskStatus | "all",
  });
}

function onProject(value: string): void {
  store.setFilters({
    projectId: (value === "all" ? "all" : value) as TaskCenterViewProjectFilter,
  });
}

type TaskCenterViewProjectFilter = import("./types").TaskCenterFilters["projectId"];

function onUpdated(value: string): void {
  store.setFilters({ updatedWithin: value as UpdatedWithin });
}

function onGroup(value: string): void {
  const group = (value === "all" ? "all" : value) as TaskGroupId | "all";
  store.setFilters({ group });
  if (props.syncHash !== false) {
    applyTaskCenterHash(store.selectedTaskId, group === "all" ? null : group);
  }
}

function focusGroup(group: TaskGroupId | "all"): void {
  store.setFilters({ group });
  if (props.syncHash !== false) {
    applyTaskCenterHash(store.selectedTaskId, group === "all" ? null : group);
  }
}

async function openTask(taskId: string): Promise<void> {
  await store.openDetail(taskId as TaskId);
  if (props.syncHash !== false) {
    const group =
      store.filters.group !== "all" ? store.filters.group : null;
    applyTaskCenterHash(taskId, group);
  }
}

function closeDrawer(): void {
  store.closeDetail();
  if (props.syncHash !== false) {
    const group =
      store.filters.group !== "all" ? store.filters.group : null;
    applyTaskCenterHash(null, group);
  }
}

function requestCancel(taskId: string): void {
  confirmCancelTaskId.value = taskId as TaskId;
  confirmCancelOpen.value = true;
  cancelFeedback.value = null;
}

async function confirmCancel(): Promise<void> {
  const id = confirmCancelTaskId.value;
  if (!id) return;
  const result = await store.cancelTask(id);
  if (!result.ok) {
    cancelFeedback.value = result.message ?? "取消失败";
  } else {
    confirmCancelOpen.value = false;
    confirmCancelTaskId.value = null;
    cancelFeedback.value = null;
  }
}

function requestRecover(taskId: string): void {
  // Recovery execution is out of scope (GAG-014). Surface entry point only.
  cancelFeedback.value = null;
  void openTask(taskId);
}

function applyRouteFromHash(): void {
  if (props.syncHash === false) return;
  const route = parseTaskCenterHash(window.location.hash);
  if (!route.active) return;
  if (route.group) {
    store.setFilters({ group: route.group });
  }
  if (route.taskId) {
    if (store.selectedTaskId !== route.taskId) {
      void store.openDetail(route.taskId as TaskId);
    }
  } else if (store.selectedTaskId) {
    store.closeDetail();
  }
}

function onHashChange(): void {
  applyRouteFromHash();
}

onMounted(async () => {
  await store.attach(props.bridge);
  if (props.syncHash !== false) {
    window.addEventListener("hashchange", onHashChange);
    applyRouteFromHash();
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("hashchange", onHashChange);
  store.detach();
});

watch(
  () => props.bridge,
  async (bridge) => {
    store.detach();
    await store.attach(bridge);
    applyRouteFromHash();
  },
);
</script>

<template>
  <section class="task-center" data-testid="task-center" aria-labelledby="task-center-title">
    <header class="task-center-header">
      <div class="title-row">
        <h1 id="task-center-title">任务中心</h1>
        <div class="counts" aria-label="任务计数">
          <Badge tone="warning">等待 {{ store.counts.needs_attention }}</Badge>
          <Badge tone="info">运行 {{ store.counts.running }}</Badge>
          <Badge tone="success">完成 {{ store.counts.completed }}</Badge>
          <Badge tone="danger">中断 {{ store.counts.failed_interrupted }}</Badge>
          <span class="count-total">共 {{ totalCount }} · 显示 {{ visibleCount }}</span>
        </div>
      </div>

      <div class="filters" role="search">
        <Input
          :model-value="store.filters.query"
          label="搜索任务"
          placeholder="标题、项目、分支…"
          data-testid="task-search"
          @update:model-value="onQuery"
        />
        <Select
          :model-value="store.filters.status"
          label="状态"
          :options="statusOptions"
          data-testid="task-filter-status"
          @update:model-value="onStatus"
        />
        <Select
          :model-value="String(store.filters.projectId)"
          label="项目"
          :options="projectFilterOptions"
          data-testid="task-filter-project"
          @update:model-value="onProject"
        />
        <Select
          :model-value="store.filters.updatedWithin"
          label="更新时间"
          :options="updatedOptions"
          data-testid="task-filter-updated"
          @update:model-value="onUpdated"
        />
        <Select
          :model-value="store.filters.group"
          label="分组"
          :options="groupOptions"
          data-testid="task-filter-group"
          @update:model-value="onGroup"
        />
      </div>

      <div class="group-chips" role="toolbar" aria-label="任务分组">
        <Button
          :variant="store.filters.group === 'all' ? 'primary' : 'ghost'"
          data-testid="group-chip-all"
          @click="focusGroup('all')"
        >
          全部
        </Button>
        <Button
          v-for="(label, id) in TASK_GROUP_LABELS"
          :key="id"
          :variant="store.filters.group === id ? 'primary' : 'ghost'"
          :data-group="id"
          :data-testid="`group-chip-${id}`"
          @click="focusGroup(id as TaskGroupId)"
        >
          {{ label }} ({{ store.counts[id as TaskGroupId] }})
        </Button>
      </div>
    </header>

    <div
      v-if="isStale"
      class="banner banner-stale"
      role="status"
      data-testid="task-stale-banner"
    >
      <span>连接已断开或数据可能过期。{{ store.errorMessage }}</span>
      <Button variant="secondary" data-testid="task-retry" @click="store.refresh()">
        重试
      </Button>
    </div>

    <div class="task-center-body">
      <div v-if="isLoading" class="state-block" data-testid="task-loading">
        <Skeleton height="88px" />
        <Skeleton height="88px" />
        <Skeleton height="88px" />
        <p role="status">正在加载任务…</p>
      </div>

      <ErrorState
        v-else-if="isError"
        title="无法加载任务"
        :detail="store.errorMessage || '未知错误'"
        data-testid="task-error"
      >
        <Button variant="primary" data-testid="task-retry" @click="store.refresh()">
          重试
        </Button>
      </ErrorState>

      <EmptyState
        v-else-if="isEmpty"
        title="还没有任务"
        detail="创建任务后，将按运行中、等待处理、已完成和失败/中断分组显示。"
        data-testid="task-empty"
      />

      <EmptyState
        v-else-if="isFilteredEmpty"
        title="没有匹配的任务"
        detail="尝试调整搜索关键词或筛选条件。"
        data-testid="task-filtered-empty"
      >
        <Button variant="secondary" @click="store.resetFilters()">清除筛选</Button>
      </EmptyState>

      <VirtualList
        v-else
        :items="listRows"
        :item-height="ITEM_HEIGHT"
        :get-key="rowKey"
        aria-label="任务列表"
        data-testid="task-list"
      >
        <template #default="{ item }">
          <div
            v-if="(item as TaskListRow).kind === 'header'"
            class="group-header"
            :data-group-header="(item as Extract<TaskListRow, { kind: 'header' }>).groupId"
          >
            <h2>
              {{ (item as Extract<TaskListRow, { kind: 'header' }>).label }}
              <span class="group-count">
                ({{ (item as Extract<TaskListRow, { kind: 'header' }>).count }})
              </span>
            </h2>
          </div>
          <TaskCard
            v-else
            :task="(item as Extract<TaskListRow, { kind: 'task' }>).task"
            :selected="store.selectedTaskId === (item as Extract<TaskListRow, { kind: 'task' }>).task.id"
            :cancel-pending="store.cancelPendingId === (item as Extract<TaskListRow, { kind: 'task' }>).task.id"
            @open="openTask"
            @cancel="requestCancel"
            @recover="requestRecover"
          />
        </template>
      </VirtualList>
    </div>

    <div
      class="sr-live"
      aria-live="polite"
      aria-atomic="true"
      data-testid="task-live-region"
    >
      {{ store.liveMessage }}
    </div>

    <TaskDetailDrawer
      :open="drawerOpen"
      :detail="store.detail"
      :loading="store.detailLoading"
      :cancel-pending="store.cancelPendingId != null && store.cancelPendingId === store.selectedTaskId"
      @update:open="(open) => !open && closeDrawer()"
      @cancel="requestCancel"
      @recover="requestRecover"
    />

    <Dialog
      :model-value="confirmCancelOpen"
      title="确认取消任务"
      description="取消会请求停止当前 Turn。已终态任务可能无法取消。"
      @update:model-value="confirmCancelOpen = $event"
    >
      <p v-if="cancelFeedback" role="alert" data-testid="cancel-feedback">{{ cancelFeedback }}</p>
      <p v-else>确定要取消该任务吗？此操作等待后端确认，不会乐观更新状态。</p>
      <template #actions>
        <Button variant="ghost" @click="confirmCancelOpen = false">返回</Button>
        <Button
          variant="danger"
          data-testid="confirm-cancel"
          :state="store.cancelPendingId ? 'loading' : 'default'"
          @click="confirmCancel"
        >
          确认取消
        </Button>
      </template>
    </Dialog>
  </section>
</template>

<style scoped>
.task-center {
  display: grid;
  grid-template-rows: auto auto minmax(0, 1fr);
  gap: var(--space-3);
  height: 100%;
  min-height: 0;
  padding: var(--space-4);
  box-sizing: border-box;
  color: var(--ctp-text);
  background: var(--ctp-base);
}
.task-center-header {
  display: grid;
  gap: var(--space-3);
}
.title-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  align-items: center;
  justify-content: space-between;
}
.title-row h1 {
  margin: 0;
  font-size: 20px;
  line-height: 28px;
}
.counts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
}
.count-total {
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.filters {
  display: grid;
  grid-template-columns: minmax(160px, 2fr) repeat(4, minmax(120px, 1fr));
  gap: var(--space-3);
}
.group-chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}
.banner {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3);
  border-radius: var(--radius-card);
}
.banner-stale {
  color: var(--ctp-text);
  background: color-mix(in srgb, var(--ctp-yellow) 16%, var(--ctp-mantle));
  border: 1px solid var(--ctp-yellow);
}
.task-center-body {
  min-height: 0;
  overflow: hidden;
}
.state-block {
  display: grid;
  gap: var(--space-3);
}
.state-block p {
  margin: 0;
  color: var(--ctp-subtext0);
}
.group-header {
  display: flex;
  align-items: flex-end;
  height: 100%;
  box-sizing: border-box;
  padding: var(--space-2) var(--space-1) var(--space-1);
}
.group-header h2 {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.group-count {
  color: var(--ctp-overlay0);
  font-weight: 500;
}
.sr-live {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
@media (max-width: 1080px) {
  .filters {
    grid-template-columns: 1fr 1fr;
  }
}
</style>
