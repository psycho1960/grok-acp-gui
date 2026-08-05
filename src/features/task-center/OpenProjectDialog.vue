<script setup lang="ts">
import { ref, watch } from "vue";
import Button from "../../shared/ui/Button.vue";
import Dialog from "../../shared/ui/Dialog.vue";
import Input from "../../shared/ui/Input.vue";
import { pickDirectory } from "../../bridge/folder-picker";

const props = defineProps<{
  open: boolean;
  pending?: boolean;
  error?: string | null;
  /** When true, show trust confirmation before emit open. */
  requireTrust?: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  open: [path: string];
  cancel: [];
}>();

const path = ref("");
const trustChecked = ref(false);
const localError = ref<string | null>(null);
const browseHint = ref<string | null>(null);

watch(
  () => props.open,
  (v) => {
    if (v) {
      path.value = "";
      trustChecked.value = false;
      localError.value = null;
      browseHint.value = null;
    }
  },
);

async function onBrowse(): Promise<void> {
  browseHint.value = null;
  const selected = await pickDirectory({ title: "选择项目文件夹" });
  if (selected.error) {
    browseHint.value = selected.error;
    return;
  }
  if (selected.path == null) {
    browseHint.value = "已取消选择文件夹";
    return;
  }
  path.value = selected.path;
}

function onCancel(): void {
  emit("update:open", false);
  emit("cancel");
}

function onSubmit(): void {
  localError.value = null;
  const trimmed = path.value.trim();
  if (!trimmed) {
    localError.value = "请输入或选择项目目录";
    return;
  }
  if (props.requireTrust !== false && !trustChecked.value) {
    localError.value = "请先确认信任此目录";
    return;
  }
  emit("open", trimmed);
}
</script>

<template>
  <Dialog
    :model-value="open"
    title="选择项目"
    description="打开本地文件夹作为工作项目。Agent 可能读写文件并执行命令。"
    data-testid="open-project-dialog"
    @update:model-value="emit('update:open', $event)"
  >
    <div class="form" data-testid="open-project-form">
      <div class="path-row">
        <Input
          v-model="path"
          label="项目路径"
          placeholder="例如 D:\work\my-repo"
          data-testid="project-path-input"
        />
        <Button
          variant="secondary"
          data-testid="project-browse"
          :disabled="pending"
          @click="onBrowse"
        >
          浏览…
        </Button>
      </div>
      <p v-if="browseHint" class="hint" role="status">{{ browseHint }}</p>

      <label class="trust">
        <input
          v-model="trustChecked"
          type="checkbox"
          data-testid="project-trust"
        />
        <span>
          我信任此目录。Agent 可能读写文件、执行命令。路径：
          <strong>{{ path.trim() || "（尚未选择）" }}</strong>
        </span>
      </label>

      <p v-if="localError || error" class="error" role="alert" data-testid="project-open-error">
        {{ localError || error }}
      </p>
    </div>

    <template #actions>
      <Button variant="ghost" data-testid="project-open-cancel" @click="onCancel">
        取消
      </Button>
      <Button
        variant="primary"
        data-testid="project-open-submit"
        :state="pending ? 'loading' : 'default'"
        :disabled="pending"
        @click="onSubmit"
      >
        打开项目
      </Button>
    </template>
  </Dialog>
</template>

<style scoped>
.form {
  display: grid;
  gap: var(--space-3);
}
.path-row {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: var(--space-2);
  align-items: end;
}
.trust {
  display: flex;
  gap: var(--space-2);
  align-items: flex-start;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  cursor: pointer;
}
.trust input {
  margin-top: 3px;
}
.trust strong {
  color: var(--ctp-text);
  word-break: break-all;
}
.hint {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.error {
  margin: 0;
  color: var(--ctp-red);
  font-size: var(--font-small);
}
</style>
