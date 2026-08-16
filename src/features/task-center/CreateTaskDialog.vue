<script setup lang="ts">
import { computed, ref, watch } from "vue";
import Button from "../../shared/ui/Button.vue";
import Dialog from "../../shared/ui/Dialog.vue";
import Input from "../../shared/ui/Input.vue";
import Select from "../../shared/ui/Select.vue";
import Textarea from "../../shared/ui/Textarea.vue";
import type { ReasoningEffort } from "../../bridge/types";
import {
  WORKSPACE_STRATEGY_OPTIONS,
  WORKTREE_NOT_READY_MESSAGE,
  workspaceStrategyForMode,
  type WorkspaceStrategy,
} from "../conversation/mode-workspace";
import Tooltip from "../../shared/ui/Tooltip.vue";
import IconButton from "../../shared/ui/IconButton.vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import { modeHelpFor } from "../../shared/ui/mode-help";
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
      workspaceStrategy: WorkspaceStrategy;
    },
  ];
  cancel: [];
}>();

const prompt = ref("");
const title = ref("");
const mode = ref("ask");
const model = ref("");
const reasoning = ref<ReasoningEffort>("medium");
const workspaceStrategy = ref<WorkspaceStrategy>("direct");
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

const workspaceOptions = computed(() =>
  WORKSPACE_STRATEGY_OPTIONS.map((option) => ({
    ...option,
    label: option.value === "worktree" ? `${option.label}（推荐）` : option.label,
  })),
);

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
  const linked = workspaceStrategyForMode(m);
  if (linked) workspaceStrategy.value = linked;
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
  const initialMessage = prompt.value.trim();
  // 标题和初始消息都可留空；首轮对话发送后由后端补全标题。
  const t = title.value.trim() || (initialMessage ? deriveTaskTitle(initialMessage) : "");
  emit("create", {
    prompt: initialMessage,
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
        <h3 class="section-title">初始对话</h3>
        <Textarea
          :model-value="prompt"
          label="初始消息（可选）"
          placeholder="可稍后在对话中描述要完成的工作"
          data-testid="create-task-prompt"
          @update:model-value="prompt = $event"
        />
        <Input
          :model-value="title"
          label="标题（可选）"
          placeholder="留空将根据首条对话自动生成"
          data-testid="create-task-title"
          @update:model-value="title = $event"
        />
      </section>

      <section class="section">
        <h3 class="section-title">模式与模型</h3>
        <div class="grid-2">
          <div class="field-with-help">
            <Select
              :model-value="mode"
              label="模式"
              :options="modeOptions"
              data-testid="create-task-mode"
              @update:model-value="mode = $event"
            />
            <Tooltip :text="modeHelpFor(mode)">
              <IconButton label="模式说明" data-testid="create-task-mode-help">
                <NamedIcon name="help" :size="14" />
              </IconButton>
            </Tooltip>
          </div>
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
            workspaceStrategy = $event as WorkspaceStrategy
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
          {{ WORKTREE_NOT_READY_MESSAGE }}
        </p>
        <p v-else-if="workspaceStrategy === 'readonly'" class="hint">
          只读策略使用项目目录，但后端会拒绝写入与非只读操作。
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
.field-with-help {
  display: grid;
  min-width: 0;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: var(--space-2);
  align-items: end;
}
.field-with-help :deep(.field) {
  min-width: 0;
}
.grid-2 {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: var(--space-4);
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
  background: var(--overlay-warning);
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
