<script setup lang="ts">
import { computed } from "vue";
import { renderSafeMarkdown, visiblePlainText } from "./markdown";

const props = defineProps<{
  source: string;
  streaming?: boolean;
}>();

const html = computed(() => renderSafeMarkdown(props.source));

async function copyVisible(): Promise<void> {
  const text = visiblePlainText(props.source);
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Clipboard may be unavailable in tests / insecure contexts.
  }
}

defineExpose({ copyVisible, html });
</script>

<template>
  <div
    class="safe-md"
    data-testid="safe-markdown"
    :data-streaming="streaming ? 'true' : undefined"
  >
    <!-- content is sanitized by renderSafeMarkdown (no raw HTML pass-through) -->
    <!-- eslint-disable-next-line vue/no-v-html -->
    <div class="safe-md-body" v-html="html" />
    <button
      v-if="source"
      type="button"
      class="copy-btn"
      data-testid="copy-message"
      @click="copyVisible"
    >
      复制
    </button>
  </div>
</template>

<style scoped>
.safe-md {
  position: relative;
  color: var(--ctp-text);
  word-break: break-word;
  overflow-wrap: anywhere;
}
.safe-md-body :deep(.md-p) {
  margin: 0 0 var(--space-2);
}
.safe-md-body :deep(.md-p:last-child) {
  margin-bottom: 0;
}
.safe-md-body :deep(.md-heading) {
  margin: var(--space-4) 0 var(--space-2);
  color: var(--ctp-text);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
}
.safe-md-body :deep(.md-heading:first-child) {
  margin-top: 0;
}
.safe-md-body :deep(.md-h1) {
  font-size: var(--heading-page);
}
.safe-md-body :deep(.md-h2) {
  font-size: var(--heading-panel);
}
.safe-md-body :deep(.md-h3),
.safe-md-body :deep(.md-h4),
.safe-md-body :deep(.md-h5),
.safe-md-body :deep(.md-h6) {
  font-size: var(--font-body);
}
.safe-md-body :deep(.md-table-wrap) {
  max-width: 100%;
  margin: var(--space-2) 0 var(--space-4);
  overflow-x: auto;
}
.safe-md-body :deep(.md-table) {
  width: 100%;
  border-spacing: 0;
  border-collapse: collapse;
  font-size: var(--font-small);
}
.safe-md-body :deep(.md-table th),
.safe-md-body :deep(.md-table td) {
  padding: var(--space-2);
  vertical-align: top;
  border: 1px solid var(--ctp-surface1);
}
.safe-md-body :deep(.md-table th) {
  color: var(--ctp-text);
  font-weight: var(--font-weight-semibold);
  background: var(--ctp-surface0);
}
.safe-md-body :deep(.md-align-left) {
  text-align: left;
}
.safe-md-body :deep(.md-align-center) {
  text-align: center;
}
.safe-md-body :deep(.md-align-right) {
  text-align: right;
}
.safe-md-body :deep(.md-list) {
  margin: var(--space-2) 0;
  padding-left: var(--space-6);
}
.safe-md-body :deep(.md-list li + li) {
  margin-top: var(--space-1);
}
.safe-md-body :deep(.md-quote) {
  margin: var(--space-2) 0;
  padding-left: var(--space-3);
  color: var(--ctp-subtext0);
  border-left: 3px solid var(--ctp-iris);
}
.safe-md-body :deep(.md-rule) {
  margin: var(--space-4) 0;
  border: 0;
  border-top: 1px solid var(--ctp-surface1);
}
.safe-md-body :deep(.md-code) {
  margin: var(--space-2) 0;
  padding: var(--space-2);
  overflow: auto;
  font-family: var(--font-mono);
  font-size: var(--font-small);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.safe-md-body :deep(.md-inline) {
  padding: 0 4px;
  font-family: var(--font-mono);
  font-size: 0.92em;
  background: var(--ctp-surface0);
  border-radius: 3px;
}
.safe-md-body :deep(a) {
  color: var(--ctp-blue);
}
.copy-btn {
  margin-top: var(--space-1);
  min-height: 28px;
  padding: 0 var(--space-2);
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  background: transparent;
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  cursor: pointer;
}
.copy-btn:hover {
  color: var(--ctp-text);
  background: var(--ctp-surface0);
}
.safe-md[data-streaming="true"] .safe-md-body::after {
  content: "▍";
  margin-left: 2px;
  color: var(--ctp-mauve);
  animation: blink 1s step-end infinite;
}
@media (prefers-reduced-motion: reduce) {
  .safe-md[data-streaming="true"] .safe-md-body::after {
    animation: none;
  }
}
@keyframes blink {
  50% {
    opacity: 0;
  }
}
</style>
