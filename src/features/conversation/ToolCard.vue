<script setup lang="ts">
import { computed } from "vue";
import Badge from "../../shared/ui/Badge.vue";
import { formatDuration } from "./tool-normalize";
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
    props.tool.input.summary,
    props.tool.result.summary,
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
    class="tool-card surface-card"
    data-testid="tool-card"
    :data-phase="tool.phase"
    :data-tool-id="tool.toolCallId"
  >
    <header class="tool-head">
      <div class="tool-title-row">
        <span class="tool-title">{{ tool.title }}</span>
        <Badge :tone="tone">{{ phaseLabel[tool.phase] }}</Badge>
        <span class="tool-kind">{{ tool.kind }}</span>
        <span class="tool-dur" data-testid="tool-duration">
          {{ formatDuration(tool.durationMs) }}
        </span>
      </div>
      <div class="tool-actions">
        <button type="button" class="linkish" data-testid="tool-copy" @click="copySummary">
          复制摘要
        </button>
        <button
          type="button"
          class="linkish"
          data-testid="tool-toggle"
          :aria-expanded="expanded"
          @click="emit('toggle')"
        >
          {{ expanded ? "收起" : "展开" }}
        </button>
      </div>
    </header>
    <p class="tool-summary" data-testid="tool-input-summary">
      <span class="label">输入</span>
      {{ tool.input.summary }}
      <Badge v-if="tool.input.redacted" tone="warning">已脱敏</Badge>
    </p>
    <p
      v-if="tool.phase !== 'pending' && tool.phase !== 'running'"
      class="tool-summary"
      data-testid="tool-result-summary"
    >
      <span class="label">结果</span>
      {{ tool.result.summary }}
      <Badge v-if="tool.result.redacted" tone="warning">已脱敏</Badge>
      <span v-if="tool.exitCode != null" class="exit">exit {{ tool.exitCode }}</span>
    </p>
    <div v-if="expanded" class="tool-details" data-testid="tool-details">
      <p v-if="tool.locations.length" class="locs">
        路径：{{ tool.locations.join(", ") }}
      </p>
      <pre v-if="tool.detailsSafe" class="details-safe">{{ tool.detailsSafe }}</pre>
      <p v-else class="muted">无额外安全详情（敏感参数默认隐藏）</p>
    </div>
  </article>
</template>

<style scoped>
.tool-card {
  padding: var(--space-3);
  display: grid;
  gap: var(--space-2);
}
.tool-head {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
  justify-content: space-between;
}
.tool-title-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
}
.tool-title {
  font-weight: 600;
  color: var(--ctp-text);
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
  gap: var(--space-2);
}
.linkish {
  min-height: 28px;
  padding: 0 var(--space-2);
  color: var(--ctp-blue);
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: var(--font-small);
}
.tool-summary {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
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
