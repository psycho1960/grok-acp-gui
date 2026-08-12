<script setup lang="ts">
import { onMounted, ref } from "vue";
import Button from "./Button.vue";

/** localStorage flag: set to "1" after dismiss (keep in sync with tests/e2e/fixtures.ts). */
const STORAGE_KEY = "gag-ui-first-use-coach-v1";

const props = defineProps<{
  /** Force show (e.g. hash #first-use) ignoring localStorage. */
  force?: boolean;
}>();

const emit = defineEmits<{ done: [] }>();

const open = ref(false);
const step = ref(0);

const steps = [
  {
    title: "选择项目",
    body: "从任务中心打开本地 Git 仓库或文件夹，作为 Agent 的工作区。",
  },
  {
    title: "创建任务",
    body: "描述目标后创建任务；默认在隔离工作区中修改代码，便于安全审查与合并。",
  },
  {
    title: "对话与审批",
    body: "在时间线中与 Agent 交互，批准权限与计划，再审查变更并集成。",
  },
] as const;

onMounted(() => {
  if (props.force) {
    open.value = true;
    return;
  }
  try {
    if (localStorage.getItem(STORAGE_KEY) !== "1") open.value = true;
  } catch {
    open.value = true;
  }
});

function finish(): void {
  try {
    localStorage.setItem(STORAGE_KEY, "1");
  } catch {
    /* ignore */
  }
  open.value = false;
  emit("done");
}

function next(): void {
  if (step.value >= steps.length - 1) finish();
  else step.value += 1;
}

function skip(): void {
  finish();
}
</script>

<template>
  <div v-if="open" class="coach-layer" data-testid="first-use-coach" role="dialog" aria-modal="true" aria-labelledby="coach-title">
    <div class="coach-card">
      <p class="coach-step">引导 {{ step + 1 }} / {{ steps.length }}</p>
      <h2 id="coach-title">{{ steps[step].title }}</h2>
      <p>{{ steps[step].body }}</p>
      <div class="coach-actions">
        <Button variant="ghost" data-testid="coach-skip" @click="skip">跳过</Button>
        <Button variant="primary" data-testid="coach-next" @click="next">
          {{ step >= steps.length - 1 ? "开始使用" : "下一步" }}
        </Button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.coach-layer {
  position: fixed;
  inset: 0;
  z-index: 35;
  display: grid;
  place-items: center;
  padding: var(--space-4);
  background: color-mix(in srgb, var(--ctp-crust) var(--backdrop-alpha), transparent);
}
.coach-card {
  width: min(440px, 100%);
  padding: var(--space-5);
  color: var(--ctp-text);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-dialog);
  box-shadow: var(--elevation-modal);
}
.coach-step {
  margin: 0 0 var(--space-2);
  color: var(--ctp-mauve);
  font-size: var(--text-sm);
  font-weight: var(--font-weight-semibold);
}
.coach-card h2 {
  margin: 0;
  font-size: var(--heading-section);
  line-height: var(--leading-tight);
}
.coach-card p {
  margin: var(--space-2) 0 0;
  color: var(--ctp-subtext0);
  line-height: var(--leading-normal);
}
.coach-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  margin-top: var(--space-5);
}
</style>
