<script setup lang="ts">
import { computed, ref, watch } from "vue";
import Button from "../../shared/ui/Button.vue";
import Dialog from "../../shared/ui/Dialog.vue";
import Input from "../../shared/ui/Input.vue";
import Select from "../../shared/ui/Select.vue";
import Textarea from "../../shared/ui/Textarea.vue";
import type { ReasoningEffort } from "../../bridge/types";
import { deriveTaskTitle } from "./title";

type ModelOption = {
  value: string;
  label: string;
  reasoningEffort?: ReasoningEffort;
};

const props = defineProps<{
  open: boolean;
  pending?: boolean;
  error?: string | null;
  projectLabel?: string;
  modelOptions?: readonly ModelOption[];
  /** Risk note when choosing direct workspace. */
  dirtyWorkspace?: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  create: [
    payload: {
      prompt: string;
      title: string;
      mode: string;
      model?: string;
      reasoning: ReasoningEffort;
      workspaceStrategy: "worktree" | "readonly" | "direct";
    },
  ];
  cancel: [];
}>();

const prompt = ref("");
const title = ref("");
const mode = ref("ask");
const model = ref("");
const reasoning = ref<ReasoningEffort>("medium");
const workspaceStrategy = ref<"worktree" | "readonly" | "direct">("direct");
const localError = ref<string | null>(null);

const modeOptions = [
  { value: "agent", label: "智能体" },
  { value: "plan", label: "计划" },
  { value: "ask", label: "问答" },
];

const reasoningOptions = [
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "max", label: "最高" },
];

const workspaceOptions = computed(() => [
  { value: "worktree", label: "隔离 Worktree（推荐）" },
  { value: "readonly", label: "只读当前目录" },
  { value: "direct", label: "当前目录（可写）" },
]);

watch(
  () => props.open,
  (v) => {
    if (v) {
      prompt.value = "";
      title.value = "";
      mode.value = "ask";
      model.value = "";
      reasoning.value = "medium";
      workspaceStrategy.value = "direct";
      localError.value = null;
    }
  },
);

watch(mode, (m) => {
  // Ask defaults to current dir; Agent/Plan to worktree
  if (m === "ask") workspaceStrategy.value = "direct";
  else if (workspaceStrategy.value === "direct") workspaceStrategy.value = "worktree";
});

watch(model, (selectedModel) => {
  const configuredEffort = props.modelOptions?.find(
    (option) => option.value === selectedModel,
  )?.reasoningEffort;
  reasoning.value = configuredEffort ?? "medium";
});

function onCancel(): void {
  emit("update:open", false);
  emit("cancel");
}

function onSubmit(): void {
  localError.value = null;
  if (!prompt.value.trim()) {
    localError.value = "任务目标为必填项";
    return;
  }
  // 标题为可选字段：留空时由首句自动提炼生成。
  const t = title.value.trim() || deriveTaskTitle(prompt.value);
  emit("create", {
    prompt: prompt.value.trim(),
    title: t.slice(0, 120),
    mode: mode.value,
    model: model.value.trim() || undefined,
    reasoning: reasoning.value,
    workspaceStrategy: workspaceStrategy.value,
  });
}
</script>

<template>
  <Dialog
    :model-value="open"
    title="新建任务"
    :description="projectLabel ? `项目：${projectLabel}` : '创建后将进入对话时间线'"
    data-testid="create-task-dialog"
    @update:model-value="emit('update:open', $event)"
  >
    <div class="form" data-testid="create-task-form">
      <section class="section">
        <h3 class="section-title">目标与标题</h3>
        <Textarea
          :model-value="prompt"
          label="任务目标（必填）"
          placeholder="描述你希望智能体完成的工作…"
          data-testid="create-task-prompt"
          @update:model-value="prompt = $event"
        />
        <Input
          :model-value="title"
          label="标题（可选）"
          placeholder="留空将根据首句自动生成"
          data-testid="create-task-title"
          @update:model-value="title = $event"
        />
      </section>

      <section class="section">
        <h3 class="section-title">模式与模型</h3>
        <div class="grid-2">
          <Select
            :model-value="mode"
            label="模式"
            :options="modeOptions"
            data-testid="create-task-mode"
            @update:model-value="mode = $event"
          />
          <Select
            :model-value="model"
            label="模型（可选）"
            :options="modelOptions ?? [{ value: '', label: '使用运行时默认模型' }]"
            data-testid="create-task-model"
            @update:model-value="model = $event"
          />
        </div>
        <Select
          :model-value="reasoning"
          label="推理强度"
          :options="reasoningOptions"
          data-testid="create-task-reasoning"
          @update:model-value="reasoning = $event as ReasoningEffort"
        />
      </section>

      <section class="section">
        <h3 class="section-title">工作目录</h3>
        <Select
          :model-value="workspaceStrategy"
          label="工作区策略"
          :options="workspaceOptions"
          data-testid="create-task-workspace"
          @update:model-value="
            workspaceStrategy = $event as 'worktree' | 'readonly' | 'direct'
          "
        />
        <p
          v-if="workspaceStrategy === 'direct'"
          class="risk"
          role="status"
          data-testid="create-task-direct-risk"
        >
          当前目录模式将直接写入项目工作区
          <template v-if="dirtyWorkspace">（工作区可能不干净，请谨慎）。</template>
          。
        </p>
        <p v-else-if="workspaceStrategy === 'worktree'" class="hint">
          受管 Worktree 将由 GAG-011 启用；当前版本不会回落到原工作区执行。
        </p>
      </section>

      <p v-if="localError || error" class="error" role="alert" data-testid="create-task-error">
        {{ localError || error }}
      </p>
    </div>

    <template #actions>
      <Button variant="ghost" data-testid="create-task-cancel" @click="onCancel">
        取消
      </Button>
      <Button
        variant="primary"
        data-testid="create-task-submit"
        :state="pending ? 'loading' : 'default'"
        :disabled="pending"
        @click="onSubmit"
      >
        创建任务
      </Button>
    </template>
  </Dialog>
</template>

<style scoped>
.form {
  display: grid;
  gap: var(--space-4);
}
.section {
  display: grid;
  gap: var(--space-2);
}
.section-title {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-2);
}
.hint {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.risk {
  margin: 0;
  padding: var(--space-2);
  color: var(--ctp-yellow);
  background: color-mix(in srgb, var(--ctp-yellow) 12%, var(--ctp-mantle));
  border: 1px solid var(--ctp-yellow);
  border-radius: var(--radius-control);
  font-size: var(--font-small);
}
.error {
  margin: 0;
  color: var(--ctp-red);
  font-size: var(--font-small);
}
@media (max-width: 640px) {
  .grid-2 {
    grid-template-columns: 1fr;
  }
}
</style>
