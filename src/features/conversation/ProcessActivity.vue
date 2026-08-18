<script setup lang="ts">
import { computed, ref, watch } from "vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import ToolCard from "./ToolCard.vue";
import { formatDuration } from "./tool-normalize";
import type { ProcessActivityItem } from "./types";

const PAGE_SIZE = 50;

const props = defineProps<{ item: ProcessActivityItem }>();
const emit = defineEmits<{
  toggle: [id: string];
  toggleTool: [id: string];
  toggleThinking: [id: string];
}>();

const visibleLimit = ref(PAGE_SIZE);
watch(
  () => props.item.id,
  () => {
    visibleLimit.value = PAGE_SIZE;
  },
);

const visibleEntries = computed(() =>
  props.item.entries.slice(0, visibleLimit.value),
);
const hiddenCount = computed(() =>
  Math.max(0, props.item.entries.length - visibleLimit.value),
);

const statusLabel = computed(() => {
  if (props.item.phase === "attention") return "需要注意";
  if (props.item.phase === "running") return "正在处理";
  return "已完成";
});

const statusIcon = computed(() => {
  if (props.item.phase === "attention") return "alert" as const;
  if (props.item.phase === "running") return "loader" as const;
  return "check" as const;
});

const summary = computed(() => {
  const parts: string[] = [];
  if (props.item.counts.reads) parts.push(`查看 ${props.item.counts.reads}`);
  if (props.item.counts.executes) parts.push(`执行 ${props.item.counts.executes}`);
  if (props.item.counts.thinking) parts.push(`思考 ${props.item.counts.thinking}`);
  if (props.item.counts.failed) parts.push(`失败 ${props.item.counts.failed}`);
  return parts.join(" · ") || `${props.item.counts.total} 项`;
});
</script>

<template>
  <article
    class="process-activity"
    :data-phase="item.phase"
    data-testid="process-activity"
  >
    <button
      type="button"
      class="process-head"
      data-testid="process-activity-toggle"
      :aria-expanded="item.expanded"
      @click="emit('toggle', item.id)"
    >
      <NamedIcon class="status-icon" :name="statusIcon" :size="14" />
      <span class="title">过程活动</span>
      <span class="status">{{ statusLabel }}</span>
      <span class="summary">{{ summary }}</span>
      <span v-if="item.durationMs != null" class="duration">
        {{ formatDuration(item.durationMs) }}
      </span>
      <NamedIcon :name="item.expanded ? 'chevronDown' : 'chevronRight'" :size="14" />
    </button>

    <div
      v-if="item.expanded"
      class="process-details"
      data-testid="process-activity-details"
      role="list"
      aria-label="过程活动明细"
    >
      <div
        v-for="entry in visibleEntries"
        :key="entry.id"
        class="process-entry"
        role="listitem"
      >
        <ToolCard
          v-if="entry.kind === 'tool'"
          :tool="entry.tool"
          :expanded="entry.expanded"
          @toggle="emit('toggleTool', entry.id)"
        />
        <template v-else-if="entry.kind === 'thinking'">
          <button
            type="button"
            class="thinking-entry"
            :aria-expanded="entry.expanded"
            data-testid="thinking-card"
            @click="emit('toggleThinking', entry.id)"
          >
            <NamedIcon :name="entry.durationMs == null ? 'loader' : 'check'" :size="14" />
            <span>{{ entry.durationMs == null ? "思考中" : "已思考" }}</span>
            <span v-if="entry.durationMs != null" class="entry-duration">
              {{ formatDuration(entry.durationMs) }}
            </span>
            <NamedIcon :name="entry.expanded ? 'chevronDown' : 'chevronRight'" :size="14" />
          </button>
          <p v-if="entry.expanded && entry.summary" class="thinking-summary">
            {{ entry.summary }}
          </p>
        </template>
        <p v-else class="activity-entry">{{ entry.detail }}</p>
      </div>
      <button
        v-if="hiddenCount > 0"
        type="button"
        class="show-more"
        data-testid="process-activity-more"
        @click="visibleLimit += PAGE_SIZE"
      >
        再显示 {{ Math.min(PAGE_SIZE, hiddenCount) }} 项（尚有 {{ hiddenCount }} 项）
      </button>
    </div>
  </article>
</template>

<style scoped>
.process-activity {
  min-width: 0;
  padding: 2px 0;
}
.process-head,
.thinking-entry {
  display: flex;
  width: 100%;
  min-height: 28px;
  gap: var(--space-2);
  align-items: center;
  padding: 0;
  color: var(--ctp-overlay0);
  background: transparent;
  border: 0;
  cursor: pointer;
  text-align: left;
}
.status-icon { flex-shrink: 0; }
.process-activity[data-phase="completed"] .status-icon { color: var(--ctp-green); }
.process-activity[data-phase="running"] .status-icon { color: var(--ctp-blue); }
.process-activity[data-phase="attention"] .status-icon { color: var(--ctp-red); }
.title {
  flex-shrink: 0;
  color: var(--ctp-subtext0);
  font-weight: 600;
}
.status { flex-shrink: 0; color: var(--ctp-overlay1); }
.summary {
  min-width: 0;
  overflow: hidden;
  color: var(--ctp-overlay0);
  font-size: var(--font-small);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.duration,
.entry-duration {
  margin-left: auto;
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: var(--font-small);
}
.process-details {
  max-height: min(52vh, 420px);
  margin: var(--space-1) 0 0 1.35rem;
  padding: var(--space-1) var(--space-2);
  overflow: auto;
  border-left: 1px solid var(--ctp-surface1);
}
.process-entry + .process-entry { margin-top: 2px; }
.thinking-summary,
.activity-entry {
  margin: 0 0 var(--space-1) 1.35rem;
  color: var(--ctp-overlay1);
  font-size: var(--font-small);
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}
.show-more {
  min-height: 28px;
  margin-top: var(--space-1);
  padding: 0 var(--space-2);
  color: var(--ctp-iris);
  background: transparent;
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  cursor: pointer;
}
</style>
