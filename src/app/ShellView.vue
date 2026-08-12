<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref } from "vue";
import AppShell from "./AppShell.vue";
import Badge from "../shared/ui/Badge.vue";
import Button from "../shared/ui/Button.vue";
import EmptyState from "../shared/ui/EmptyState.vue";
import CommandPalette, { type CommandItem } from "../shared/ui/CommandPalette.vue";
import FirstUseCoach from "../shared/ui/FirstUseCoach.vue";
import IconButton from "../shared/ui/IconButton.vue";
import NamedIcon from "../shared/ui/NamedIcon.vue";
import ShortcutHelp from "../shared/ui/ShortcutHelp.vue";
import StatusIcon from "../shared/ui/StatusIcon.vue";
import type { IconName } from "../shared/ui/icons";
import { createDesktopBridge } from "../bridge/client";
import type { DesktopBridge, TaskId } from "../bridge/types";
import TaskCenterView from "../features/task-center/TaskCenterView.vue";
import {
  applyTaskCenterHash,
  parseTaskCenterHash,
} from "../features/task-center/hash-route";
import { TASK_GROUP_LABELS, type TaskGroupId } from "../features/task-center/types";
import ConversationView from "../features/conversation/ConversationView.vue";
import {
  applyConversationHash,
  parseConversationHash,
} from "../features/conversation/hash-route";
import { createStatefulTaskCenterBridge } from "../features/task-center/stateful-fake-bridge";
import { useTaskCenterStore } from "../features/task-center/task-center-store";
import WorktreePanel from "../features/worktrees/WorktreePanel.vue";
import ReviewView from "../features/review/ReviewView.vue";
import RecoveryCenter from "../features/worktrees/RecoveryCenter.vue";

defineProps<{ dataVersion?: string }>();

const inspectorOpen = ref(true);
const shortcutOpen = ref(false);
const commandOpen = ref(false);
const routeHash = ref(typeof window !== "undefined" ? window.location.hash : "");
const conversationRoute = computed(() => parseConversationHash(routeHash.value));
const showConversation = computed(() => conversationRoute.value.active);
const reviewTaskId = computed(() => {
  const match = routeHash.value.match(/^#review\/([^/?#]+)$/);
  if (!match) return undefined;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return undefined;
  }
});
const showReview = computed(() => Boolean(reviewTaskId.value));
const showRecovery = computed(() => routeHash.value === "#recovery");
const showTaskCenter = computed(() => {
  if (showConversation.value || showReview.value || showRecovery.value) return false;
  const route = parseTaskCenterHash(routeHash.value);
  return (
    route.active ||
    routeHash.value === "" ||
    routeHash.value === "#" ||
    routeHash.value === "#first-use"
  );
});
/** Only force the coach when explicitly requested; #first-use is the empty-shell demo hash. */
const forceCoach = computed(() => routeHash.value === "#coach");

function resolveBridge(): DesktopBridge {
  try {
    if (
      typeof window !== "undefined" &&
      ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
    ) {
      return createDesktopBridge();
    }
  } catch {
    // fall through
  }
  return createStatefulTaskCenterBridge();
}

const bridge = ref<DesktopBridge>(resolveBridge());
const taskStore = useTaskCenterStore();

function onHashChange(): void {
  routeHash.value = window.location.hash;
}

function onGlobalKeydown(event: KeyboardEvent): void {
  const target = event.target;
  const inField =
    target instanceof HTMLElement &&
    (target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable);

  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    commandOpen.value = true;
    return;
  }
  if (event.key === "?" && !event.ctrlKey && !event.metaKey && !event.altKey) {
    if (inField) return;
    event.preventDefault();
    shortcutOpen.value = true;
  }
}

onMounted(() => {
  window.addEventListener("hashchange", onHashChange);
  window.addEventListener("keydown", onGlobalKeydown);
});

onBeforeUnmount(() => {
  window.removeEventListener("hashchange", onHashChange);
  window.removeEventListener("keydown", onGlobalKeydown);
});

function goTaskCenter(group?: TaskGroupId): void {
  applyTaskCenterHash(null, group ?? null);
}

function goConversation(taskId?: string): void {
  const target = taskId ?? taskStore.selectedTaskId;
  if (target) applyConversationHash(target);
}

function goReview(taskId?: string): void {
  const target = taskId ?? taskStore.selectedTaskId;
  if (target) window.location.hash = `#review/${encodeURIComponent(target)}`;
}

function goRecovery(): void {
  window.location.hash = "#recovery";
}

const taskRoute = computed(() => parseTaskCenterHash(routeHash.value));
const activeGroup = computed(() => taskRoute.value.group ?? "all");

const groupIcons: Record<TaskGroupId, IconName> = {
  needs_attention: "clock",
  running: "play",
  completed: "check",
  failed_interrupted: "alert",
};

function navItem(
  opts: {
    label: string;
    icon: IconName;
    active: boolean;
    onClick: () => void;
    testId?: string;
    disabled?: boolean;
    count?: number;
    dataGroup?: string;
  },
) {
  return h(
    "button",
    {
      class: ["nav-item", { active: opts.active }],
      type: "button",
      disabled: opts.disabled,
      "data-testid": opts.testId,
      "data-group": opts.dataGroup,
      "aria-current": opts.active ? "page" : undefined,
      onClick: opts.onClick,
    },
    [
      h(NamedIcon, { name: opts.icon, size: 16 }),
      h("span", { class: "nav-label" }, opts.label),
      opts.count !== undefined
        ? h("span", { class: "nav-count" }, String(opts.count))
        : null,
    ],
  );
}

const left = computed(() =>
  h("nav", { class: "nav-list", "aria-label": "主导航" }, [
    h("p", { class: "nav-section" }, "任务"),
    navItem({
      label: "全部任务",
      icon: "list",
      active: showTaskCenter.value && activeGroup.value === "all",
      onClick: () => goTaskCenter(),
      testId: "nav-all-tasks",
    }),
    ...Object.entries(TASK_GROUP_LABELS).map(([id, label]) =>
      navItem({
        label,
        icon: groupIcons[id as TaskGroupId],
        active: showTaskCenter.value && activeGroup.value === id,
        onClick: () => goTaskCenter(id as TaskGroupId),
        dataGroup: id,
        count: taskStore.counts[id as TaskGroupId],
        testId: `group-chip-${id}`,
      }),
    ),
    h("p", { class: "nav-section" }, "工具"),
    navItem({
      label: "对话时间线",
      icon: "message",
      active: showConversation.value,
      onClick: () => goConversation(),
      testId: "nav-conversation",
      disabled: !taskStore.selectedTaskId && !conversationRoute.value.taskId,
    }),
    navItem({
      label: "变更审查",
      icon: "gitBranch",
      active: showReview.value,
      onClick: () => goReview(),
      testId: "nav-review",
      disabled: !taskStore.selectedTaskId && !reviewTaskId.value,
    }),
    h("p", { class: "nav-section" }, "系统"),
    navItem({
      label: "恢复中心",
      icon: "shield",
      active: showRecovery.value,
      onClick: goRecovery,
      testId: "nav-recovery",
    }),
  ]),
);

const main = computed(() => {
  if (showRecovery.value) {
    return h(RecoveryCenter, { bridge: bridge.value });
  }
  if (showReview.value && reviewTaskId.value) {
    return h(ReviewView, { bridge: bridge.value, taskId: reviewTaskId.value as TaskId });
  }
  if (showConversation.value) {
    const routeTaskId = conversationRoute.value.taskId;
    if (routeTaskId) {
      return h(ConversationView, {
        bridge: bridge.value,
        taskId: routeTaskId as TaskId,
        focusSeq: conversationRoute.value.eventSeq,
      });
    }
  }
  if (showTaskCenter.value) {
    return h(TaskCenterView, {
      bridge: bridge.value,
      syncHash: true,
    });
  }
  return h("div", { class: "main-empty" }, [
    h(
      EmptyState,
      {
        title: "没有打开的任务",
        detail: "选择一个任务后，对话、计划和工具时间线将显示在这里。",
      },
      {
        default: () =>
          h(
            Button,
            {
              class: "new-task",
              variant: "primary",
              onClick: () => applyTaskCenterHash(null),
            },
            { default: () => "新建任务" },
          ),
      },
    ),
  ]);
});

const inspector = computed(() => {
  if (showRecovery.value) {
    return h("section", { class: "inspector-content" }, [
      h("h2", "恢复说明"),
      h(StatusIcon, { status: "waiting", label: "等待用户批准" }),
      h("p", "扫描不会清理资源。每个操作都绑定证据 revision，状态变化会拒绝执行。"),
      h(Badge, { tone: "warning" }, { default: () => "Fail closed" }),
    ]);
  }
  const taskId = reviewTaskId.value ?? taskStore.selectedTaskId;
  if (taskId) {
    return h(WorktreePanel, {
      bridge: bridge.value,
      taskId: taskId as TaskId,
    });
  }
  return h("section", { class: "inspector-content" }, [
    h("h2", "检查器"),
    h(StatusIcon, { status: "waiting", label: "等待选择任务" }),
    h("p", "变更、Diff、制品和 Worktree 信息将在选择任务后显示。"),
    h(Badge, { tone: "neutral" }, { default: () => "空状态" }),
  ]);
});

const selectedTask = computed(() => {
  const id =
    reviewTaskId.value ??
    conversationRoute.value.taskId ??
    taskStore.selectedTaskId;
  if (!id) return undefined;
  return taskStore.allTasks.find((task) => task.id === id);
});

const connectionLabel = computed(() => {
  if (taskStore.loadState === "stale") return "连接中断";
  if (taskStore.loadState === "error") return "连接错误";
  if (taskStore.loadState === "loading") return "加载中";
  return "已连接";
});

const statusBar = computed(() => {
  const project = taskStore.activeProject;
  const path = project?.displayPath || project?.path || "未选择项目";
  const session = selectedTask.value?.sessionState || selectedTask.value?.sessionId || "—";
  const model = selectedTask.value?.model || taskStore.modelOptions[0]?.label || "默认模型";
  return h("div", { class: "status-bar-grid", "data-testid": "status-bar" }, [
    h("span", { class: "status-left", title: path }, path),
    h(
      "span",
      { class: "status-mid" },
      `session ${session} · ${connectionLabel.value}`,
    ),
    h("span", { class: "status-right" }, model),
  ]);
});

const projectLabel = computed(() => {
  const project = taskStore.activeProject;
  return project?.displayPath || project?.path || "Project";
});

const workspaceLabel = computed(() => {
  const project = taskStore.activeProject;
  return project?.repoRoot || project?.path || "No workspace selected";
});

const breadcrumbTrail = computed(() => {
  const crumbs: { label: string; onClick?: () => void }[] = [
    { label: projectLabel.value, onClick: () => goTaskCenter() },
  ];
  if (showRecovery.value) {
    crumbs.push({ label: "恢复中心" });
  } else if (showReview.value) {
    crumbs.push({ label: "任务中心", onClick: () => goTaskCenter() });
    crumbs.push({
      label: `审查：${selectedTask.value?.title || reviewTaskId.value || "任务"}`,
    });
  } else if (showConversation.value) {
    crumbs.push({ label: "任务中心", onClick: () => goTaskCenter() });
    crumbs.push({
      label: `对话：${selectedTask.value?.title || conversationRoute.value.taskId || "任务"}`,
    });
  } else {
    crumbs.push({ label: "任务中心" });
  }
  return crumbs;
});

const commandItems = computed((): CommandItem[] => {
  const nav: CommandItem[] = [
    {
      id: "nav-tasks",
      label: "打开任务中心",
      group: "页面",
      icon: "list",
      keywords: "task center 任务",
      run: () => goTaskCenter(),
    },
    {
      id: "nav-recovery",
      label: "打开恢复中心",
      group: "页面",
      icon: "shield",
      keywords: "recovery 恢复",
      run: () => goRecovery(),
    },
    {
      id: "nav-shortcuts",
      label: "键盘快捷键",
      group: "页面",
      icon: "help",
      run: () => {
        shortcutOpen.value = true;
      },
    },
  ];
  if (taskStore.selectedTaskId) {
    nav.push(
      {
        id: "nav-conversation",
        label: "打开当前对话",
        group: "页面",
        icon: "message",
        run: () => goConversation(),
      },
      {
        id: "nav-review",
        label: "打开变更审查",
        group: "页面",
        icon: "gitBranch",
        run: () => goReview(),
      },
    );
  }
  const tasks: CommandItem[] = taskStore.allTasks.slice(0, 40).map((task) => ({
    id: `task-${task.id}`,
    label: task.title || String(task.id),
    hint: task.status,
    group: "任务",
    icon: "list" as IconName,
    keywords: `${task.id} ${task.projectLabel ?? ""}`,
    run: () => {
      void taskStore.openDetail(task.id);
      goConversation(task.id);
    },
  }));
  return [...nav, ...tasks];
});
</script>

<template>
  <AppShell
    :left="left"
    :main="main"
    :inspector="inspector"
    :inspector-open="inspectorOpen"
    :status-bar="statusBar"
    :project-label="projectLabel"
    :workspace-label="workspaceLabel"
    @update:inspector-open="inspectorOpen = $event"
  >
    <template #topbar>
      <nav class="breadcrumb" aria-label="面包屑" data-testid="topbar-breadcrumb">
        <template v-for="(crumb, index) in breadcrumbTrail" :key="`${crumb.label}-${index}`">
          <span v-if="index > 0" class="crumb-sep" aria-hidden="true">/</span>
          <button
            v-if="crumb.onClick && index < breadcrumbTrail.length - 1"
            type="button"
            class="crumb-link"
            :data-testid="index === 0 ? 'topbar-project' : undefined"
            :title="crumb.label"
            @click="crumb.onClick"
          >
            {{ crumb.label }}
          </button>
          <span
            v-else
            class="crumb-current"
            :data-testid="index === 0 ? 'topbar-project' : undefined"
            :title="crumb.label"
          >
            {{ crumb.label }}
          </span>
        </template>
      </nav>
      <span class="branch" data-testid="topbar-workspace" :title="workspaceLabel">{{ workspaceLabel }}</span>
      <span class="topbar-spacer" />
      <IconButton label="命令面板 Ctrl+K" data-testid="open-command-palette" @click="commandOpen = true">
        <NamedIcon name="list" :size="16" />
      </IconButton>
      <IconButton label="键盘快捷键" data-testid="open-shortcuts" @click="shortcutOpen = true">
        <NamedIcon name="help" :size="16" />
      </IconButton>
    </template>
  </AppShell>
  <FirstUseCoach :force="forceCoach" />
  <ShortcutHelp v-model="shortcutOpen" />
  <CommandPalette v-model="commandOpen" :items="commandItems" />
</template>

<style scoped>
.nav-list {
  display: grid;
  gap: var(--space-1);
}
.nav-section {
  margin: var(--space-3) 0 var(--space-1);
  color: var(--ctp-subtext0);
  font-size: var(--text-xs);
  letter-spacing: 0.04em;
}
.nav-section:first-child {
  margin-top: 0;
}
.nav-list :deep(.nav-item) {
  display: grid;
  grid-template-columns: 16px 1fr auto;
  gap: var(--space-2);
  align-items: center;
  min-height: 32px;
  padding: 0 var(--space-2) 0 calc(var(--space-2) + 3px);
  color: var(--ctp-subtext0);
  text-align: left;
  background: transparent;
  border: 1px solid transparent;
  border-left: 3px solid transparent;
  border-radius: var(--radius-control);
  cursor: pointer;
}
.nav-list :deep(.nav-item:hover) {
  color: var(--ctp-text);
  background: var(--ctp-surface0);
}
.nav-list :deep(.nav-item.active) {
  color: var(--ctp-text);
  background: var(--overlay-active);
  border-left-color: var(--ctp-mauve);
}
.nav-list :deep(.nav-item:disabled) {
  opacity: 0.45;
  cursor: not-allowed;
}
.nav-list :deep(.nav-count) {
  color: var(--ctp-overlay0);
  font-size: var(--text-xs);
}
.nav-list :deep(h2),
.inspector-content h2 {
  margin: 0 0 var(--space-2);
  color: var(--ctp-text);
  font-size: var(--heading-panel);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
}
.main-empty {
  display: grid;
  min-height: 100%;
  place-items: center;
}
.new-task {
  margin-top: var(--space-4);
}
.inspector-content {
  display: grid;
  gap: var(--space-3);
}
.inspector-content p {
  margin: 0;
  color: var(--ctp-subtext0);
}
.breadcrumb {
  display: flex;
  min-width: 0;
  max-width: min(560px, 50vw);
  align-items: center;
  gap: var(--space-1);
  overflow: hidden;
}
.crumb-sep {
  color: var(--ctp-overlay0);
  flex-shrink: 0;
}
.crumb-link,
.crumb-current {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--font-small);
}
.crumb-link {
  padding: 0;
  color: var(--ctp-subtext0);
  cursor: pointer;
  background: transparent;
  border: 0;
}
.crumb-link:hover {
  color: var(--ctp-mauve);
}
.crumb-current {
  color: var(--ctp-text);
  font-weight: var(--font-weight-semibold);
}
.branch {
  min-width: 0;
  max-width: 280px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.topbar-spacer {
  flex: 1;
}
:deep(.status-bar-grid) {
  display: grid;
  grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) minmax(0, 0.8fr);
  gap: var(--space-3);
  width: 100%;
  align-items: center;
}
:deep(.status-left),
:deep(.status-mid),
:deep(.status-right) {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
:deep(.status-mid) {
  text-align: center;
}
:deep(.status-right) {
  text-align: right;
}
</style>
