<script setup lang="ts">
import { computed, ref } from "vue";
import Button from "../../shared/ui/Button.vue";
import type { ComposerAttachment, ComposerCapabilities } from "./types";

const props = defineProps<{
  modelValue: string;
  capabilities: ComposerCapabilities;
  sendError?: string | null;
  sendPending?: boolean;
  attachmentPending?: boolean;
  attachments?: ComposerAttachment[];
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  send: [];
  cancel: [];
  addAttachments: [];
  dropAttachments: [paths: string[]];
  removeAttachment: [artifactId: string];
}>();

const textarea = ref<HTMLTextAreaElement | null>(null);
const dropActive = ref(false);
const hasContent = computed(() => props.modelValue.trim().length > 0 || (props.attachments?.length ?? 0) > 0);

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    if (props.capabilities.canCancel) {
      event.preventDefault();
      emit("cancel");
    }
    return;
  }
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    if (props.capabilities.canSend && hasContent.value) {
      emit("send");
    }
  }
}

function pathsFromTransfer(transfer: DataTransfer | null): string[] {
  return Array.from(transfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path)
    .filter((path): path is string => typeof path === "string" && path.length > 0);
}

function onDrop(event: DragEvent | ClipboardEvent): void {
  event.preventDefault();
  dropActive.value = false;
  const paths = "dataTransfer" in event ? pathsFromTransfer(event.dataTransfer) : pathsFromTransfer(event.clipboardData);
  if (paths.length) emit("dropAttachments", paths);
}

defineExpose({ textarea, focus: () => textarea.value?.focus() });
</script>

<template>
  <footer
    class="composer"
    :class="{ 'drop-active': dropActive }"
    data-testid="composer"
    aria-label="消息输入"
    @dragover.prevent="dropActive = true"
    @dragleave="dropActive = false"
    @drop="onDrop"
  >
    <div v-if="dropActive" class="drop-zone" aria-hidden="true">松开即可添加图片</div>
    <p v-if="capabilities.disabledReason" class="disabled-reason" role="status">
      {{ capabilities.disabledReason }}
    </p>
    <ul v-if="attachments?.length" class="attachment-list" aria-label="待发送附件">
      <li v-for="attachment in attachments" :key="attachment.artifactId">
        <span aria-hidden="true">▧</span><span>{{ attachment.displayName }}</span>
        <span class="attachment-size">{{ Math.ceil(attachment.bytes / 1024) }} KiB</span>
        <button type="button" :aria-label="`移除附件 ${attachment.displayName}`" :disabled="sendPending" @click="emit('removeAttachment', attachment.artifactId)">×</button>
      </li>
    </ul>
    <p v-if="sendError" class="send-error" role="alert" data-testid="send-error">
      {{ sendError }}
    </p>
    <div class="row">
      <label class="sr-only" for="composer-input">消息</label>
      <textarea
        id="composer-input"
        ref="textarea"
        class="input"
        data-testid="composer-input"
        rows="3"
        :value="modelValue"
        :disabled="!capabilities.canSend && !capabilities.canCancel"
        :placeholder="
          capabilities.bridgeOnline
            ? '输入消息 · Enter 发送 · Shift+Enter 换行 · Esc 停止'
            : 'Bridge 离线 — 草稿仍会保留'
        "
        @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
        @keydown="onKeydown"
        @paste="onDrop"
      />
      <div class="actions">
        <Button data-testid="composer-add-attachment" :disabled="!capabilities.canSend || sendPending" :state="attachmentPending ? 'loading' : 'default'" @click="emit('addAttachments')">添加图片</Button>
        <Button
          v-if="capabilities.canCancel"
          variant="danger"
          data-testid="composer-stop"
          :state="sendPending ? 'loading' : 'default'"
          @click="emit('cancel')"
        >
          停止
        </Button>
        <Button
          variant="primary"
          data-testid="composer-send"
          :disabled="!capabilities.canSend || !hasContent"
          :state="sendPending ? 'loading' : 'default'"
          @click="emit('send')"
        >
          发送
        </Button>
      </div>
    </div>
  </footer>
</template>

<style scoped>
.composer {
  position: relative;
  flex-shrink: 0;
  padding: var(--space-3);
  border-top: 1px solid var(--ctp-surface0);
  background: var(--ctp-mantle);
}
.composer.drop-active { outline: 2px solid var(--ctp-blue); outline-offset: -2px; }
.drop-zone { position: absolute; inset: 0; z-index: 2; display: grid; place-items: center; color: var(--ctp-text); background: color-mix(in srgb, var(--ctp-mantle) 90%, transparent); border: 2px dashed var(--ctp-blue); pointer-events: none; }
.attachment-list { display: flex; flex-wrap: wrap; gap: var(--space-2); padding: 0; margin: 0 0 var(--space-2); list-style: none; }
.attachment-list li { display: inline-flex; max-width: 100%; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; background: var(--ctp-surface0); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); }
.attachment-list button { color: var(--ctp-subtext0); cursor: pointer; background: transparent; border: 0; }
.attachment-list button:focus-visible { outline: 2px solid var(--ctp-blue); outline-offset: 2px; }
.attachment-size { color: var(--ctp-subtext0); font-size: var(--font-small); }
.row {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: var(--space-2);
  align-items: end;
}
.input {
  width: 100%;
  min-height: 72px;
  max-height: 200px;
  padding: var(--space-2);
  resize: vertical;
  color: var(--ctp-text);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.input:disabled {
  color: var(--ctp-overlay0);
  cursor: not-allowed;
}
.actions {
  display: grid;
  gap: var(--space-2);
}
.disabled-reason,
.send-error {
  margin: 0 0 var(--space-2);
  font-size: var(--font-small);
}
.disabled-reason {
  color: var(--ctp-subtext0);
}
.send-error {
  color: var(--ctp-red);
}
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
</style>
