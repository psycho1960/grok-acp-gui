<script setup lang="ts">
import { computed } from "vue";
import Badge from "../../shared/ui/Badge.vue";
import IconButton from "../../shared/ui/IconButton.vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import {
  collapsedToolSummary,
  displayToolSummary,
  formatDuration,
} from "./tool-normalize";
import type { ToolCallView, ToolPhase } from "./types";

const props = defineProps<{
  tool: ToolCallView;
  expanded: boolean;
}>();

const emit = defineEmits<{ toggle: [] }>();

const tone = computed(() => {
  switch (props.tool.phase) {
    case "completed":
      return "success" as const;
    case "failed":
      return "danger" as const;
    case "cancelled":
      return "warning" as const;
    case "running":
      return "info" as const;
    default:
      return "neutral" as const;
  }
});

const oneLine = computed(() => collapsedToolSummary(props.tool));
const inputSummary = computed(() => displayToolSummary(props.tool.input.summary));
const resultSummary = computed(() => displayToolSummary(props.tool.result.summary));

const phaseIcon = computed(() => {
  switch (props.tool.phase) {
    case "completed":
      return "check" as const;
    case "failed":
      return "alert" as const;
    case "cancelled":
      return "x" as const;
    case "running":
      return "loader" as const;
    default:
      return "circle" as const;
  }
});

const phaseLabel: Record<ToolPhase, string> = {
  pending: "排队",
  running: "运行中",
  completed: "完成",
  failed: "失败",
  cancelled: "已取消",
};

async function copySummary(): Promise<void> {
  const parts = [
    props.tool.title,
    phaseLabel[props.tool.phase],
    inputSummary.value,
    resultSummary.value,
  ];
  try {
    await navigator.clipboard.writeText(parts.filter(Boolean).join(" · "));
  } catch {
    // ignore
  }
}
</script>

<template>
  <article
    class="tool-card"
    data-testid="tool-card"
    :data-phase="tool.phase"
    :data-tool-id="tool.toolCallId"
  >
    <header class="tool-head">
      <div class="tool-title-row">
        <NamedIcon class="tool-phase-icon" :name="phaseIcon" :size="14" />
        <span class="tool-title">{{ tool.title }}</span>
        <p v-if="!expanded && oneLine" class="tool-one-line">{{ oneLine }}</p>
      </div>
      <div class="tool-actions" data-testid="tool-actions">
        <span class="tool-dur" data-testid="tool-duration">
          {{ formatDuration(tool.durationMs) }}
        </span>
        <IconButton label="复制摘要" data-testid="tool-copy" @click="copySummary">
          <NamedIcon name="copy" :size="14" />
        </IconButton>
        <IconButton
          :label="expanded ? '收起' : '展开'"
          data-testid="tool-toggle"
          :aria-expanded="expanded"
          @click="emit('toggle')"
        >
          <NamedIcon :name="expanded ? 'chevronDown' : 'chevronRight'" :size="14" />
        </IconButton>
      </div>
    </header>
    <template v-if="expanded">
      <p class="tool-summary" data-testid="tool-phase-label">
        <span class="label">状态</span>
        <Badge :tone="tone">{{ phaseLabel[tool.phase] }}</Badge>
      </p>
      <p class="tool-summary" data-testid="tool-input-summary">
        <span class="label">输入</span>
        {{ inputSummary }}
        <Badge v-if="tool.input.redacted" tone="warning">敏感值已隐藏</Badge>
      </p>
      <p
        v-if="tool.phase !== 'pending' && tool.phase !== 'running'"
        class="tool-summary"
        data-testid="tool-result-summary"
      >
        <span class="label">结果</span>
        {{ resultSummary }}
        <Badge v-if="tool.result.redacted" tone="warning">敏感值已隐藏</Badge>
        <span v-if="tool.exitCode != null" class="exit">exit {{ tool.exitCode }}</span>
      </p>
    </template>
    <div v-if="expanded" class="tool-details" data-testid="tool-details">
      <p v-if="tool.locations.length" class="locs">
        路径：{{ tool.locations.join(", ") }}
      </p>
      <pre v-if="tool.detailsSafe" class="details-safe">{{ tool.detailsSafe }}</pre>
      <p v-else class="muted">没有更多详情</p>
    </div>
  </article>
</template>

<style scoped>
.tool-card {
  display: grid;
  gap: var(--space-1);
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  padding: 2px 0;
}
.tool-head {
  display: flex;
  flex-wrap: nowrap;
  gap: var(--space-2);
  align-items: center;
  justify-content: space-between;
  min-width: 0;
}
.tool-title-row {
  display: flex;
  flex: 1 1 auto;
  flex-wrap: nowrap;
  gap: var(--space-2);
  align-items: center;
  min-width: 0;
}
.tool-phase-icon {
  flex-shrink: 0;
  color: var(--ctp-overlay0);
}
.tool-card[data-phase="completed"] .tool-phase-icon { color: var(--ctp-green); }
.tool-card[data-phase="failed"] .tool-phase-icon { color: var(--ctp-red); }
.tool-card[data-phase="running"] .tool-phase-icon { color: var(--ctp-blue); }
.tool-card[data-phase="cancelled"] .tool-phase-icon { color: var(--ctp-yellow); }
.tool-title {
  min-width: 0;
  flex: 0 1 auto;
  overflow: hidden;
  color: var(--ctp-subtext0);
  font-family: var(--font-mono);
  font-size: var(--font-small);
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tool-kind,
.tool-dur,
.muted,
.exit {
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.tool-actions {
  display: flex;
  flex-shrink: 0;
  gap: var(--space-1);
  align-items: center;
  margin-left: auto;
}
.tool-one-line {
  flex: 1 1 auto;
  margin: 0;
  min-width: 0;
  overflow: hidden;
  color: var(--ctp-overlay0);
  font-size: var(--font-small);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tool-summary {
  margin: 0;
  min-width: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}
.tool-summary .label {
  margin-right: var(--space-1);
  color: var(--ctp-overlay1);
}
.tool-details {
  padding-top: var(--space-2);
  border-top: 1px solid var(--ctp-surface0);
}
.details-safe {
  margin: 0;
  padding: var(--space-2);
  overflow: auto;
  max-height: 200px;
  font-family: var(--font-mono);
  font-size: var(--font-small);
  background: var(--ctp-surface0);
  border-radius: var(--radius-control);
}
.locs {
  margin: 0 0 var(--space-1);
  font-size: var(--font-small);
  color: var(--ctp-subtext0);
}
</style>
