<script setup lang="ts">
import { computed, ref } from "vue";
import Button from "./Button.vue";
import NamedIcon from "./NamedIcon.vue";
import { copyText, mapErrorMessage } from "./error-map";

const props = withDefaults(
  defineProps<{
    title?: string;
    detail: string;
    /** When true, map technical detail to friendlier summary. */
    friendly?: boolean;
  }>(),
  { title: undefined, friendly: true },
);

const copied = ref(false);
const mapped = computed(() =>
  props.friendly
    ? mapErrorMessage(props.detail, props.title ?? "出现错误")
    : {
        title: props.title ?? "出现错误",
        summary: props.detail,
        suggestion: undefined as string | undefined,
        raw: props.detail,
      },
);

const displayTitle = computed(() => props.title ?? mapped.value.title);

async function onCopy(): Promise<void> {
  const body = [
    displayTitle.value,
    mapped.value.summary,
    mapped.value.suggestion,
    "---",
    mapped.value.raw,
  ]
    .filter(Boolean)
    .join("\n");
  const ok = await copyText(body);
  copied.value = ok;
  if (ok) window.setTimeout(() => {
    copied.value = false;
  }, 2000);
}
</script>

<template>
  <section class="error-state" role="alert" data-testid="error-state">
    <span class="error-icon" aria-hidden="true">
      <NamedIcon name="alert" :size="14" />
    </span>
    <div class="body">
      <h2>{{ displayTitle }}</h2>
      <p class="summary">{{ mapped.summary }}</p>
      <p v-if="mapped.suggestion" class="suggestion">{{ mapped.suggestion }}</p>
      <details v-if="friendly && mapped.raw !== mapped.summary" class="raw">
        <summary>技术详情</summary>
        <pre>{{ mapped.raw }}</pre>
      </details>
      <div class="actions">
        <slot />
        <Button
          variant="ghost"
          data-testid="error-copy-detail"
          @click="onCopy"
        >
          {{ copied ? "已复制" : "复制错误详情" }}
        </Button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.error-state {
  display: flex;
  gap: var(--space-3);
  padding: var(--space-4);
  color: var(--ctp-text);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-red);
  border-radius: var(--radius-card);
}
.error-icon {
  display: grid;
  width: 24px;
  height: 24px;
  place-items: center;
  color: var(--ctp-crust);
  background: var(--ctp-red);
  border-radius: 50%;
  flex-shrink: 0;
}
.body {
  min-width: 0;
  flex: 1;
  display: grid;
  gap: var(--space-2);
}
.error-state h2,
.error-state p {
  margin: 0;
}
.error-state h2 {
  font-size: var(--heading-panel);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
}
.summary {
  color: var(--ctp-subtext0);
}
.suggestion {
  color: var(--ctp-text);
  font-size: var(--font-small);
}
.raw {
  font-size: var(--font-small);
  color: var(--ctp-subtext0);
}
.raw pre {
  margin: var(--space-1) 0 0;
  padding: var(--space-2);
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--ctp-overlay0);
  background: var(--ctp-surface0);
  border-radius: var(--radius-control);
  font-family: var(--font-mono);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  align-items: center;
  margin-top: var(--space-1);
}
</style>
