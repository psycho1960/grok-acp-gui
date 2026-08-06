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
  dropActive?: boolean;
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
const htmlDropActive = ref(false);
const hasContent = computed(
  () => props.modelValue.trim().length > 0 || (props.attachments?.length ?? 0) > 0,
);
const showDropZone = computed(() => props.dropActive || htmlDropActive.value);

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
    if (
      props.capabilities.canSend &&
      hasContent.value
    ) {
      emit("send");
    }
  }
}

function onDragOver(event: DragEvent): void {
  if (!props.capabilities.canSend || props.sendPending) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
  htmlDropActive.value = true;
}

function onDragLeave(event: DragEvent): void {
  if (event.currentTarget === event.target) htmlDropActive.value = false;
}

function onDrop(event: DragEvent): void {
  event.preventDefault();
  htmlDropActive.value = false;
  if (!props.capabilities.canSend || props.sendPending) return;
  const paths = Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path)
    .filter((path): path is string => typeof path === "string" && path.length > 0);
  if (paths.length > 0) emit("dropAttachments", paths);
}

defineExpose({ textarea, focus: () => textarea.value?.focus() });
</script>

<template>
  <footer
    class="composer"
    :class="{ 'is-drop-active': showDropZone }"
    data-testid="composer"
    aria-label="消息输入"
    @dragenter="onDragOver"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <div v-if="showDropZone" class="drop-zone" data-testid="composer-drop-zone" aria-hidden="true">
      <span class="drop-zone-icon">＋</span>
      <strong>松开即可添加图片</strong>
      <span>PNG、JPEG、GIF 或 WebP，单张不超过 20 MiB</span>
    </div>
    <p v-if="capabilities.disabledReason" class="disabled-reason" role="status">
      {{ capabilities.disabledReason }}
    </p>
    <p v-if="sendError" class="send-error" role="alert" data-testid="send-error">
      {{ sendError }}
    </p>
    <ul v-if="attachments?.length" class="attachment-list" aria-label="待发送附件">
      <li v-for="attachment in attachments" :key="attachment.artifactId" class="attachment-chip">
        <span aria-hidden="true">▧</span>
        <span class="attachment-name">{{ attachment.displayName }}</span>
        <span class="attachment-size">{{ Math.ceil(attachment.bytes / 1024) }} KiB</span>
        <button
          type="button"
          class="attachment-remove"
          :aria-label="`移除附件 ${attachment.displayName}`"
          :disabled="sendPending"
          @click="emit('removeAttachment', attachment.artifactId)"
        >
          ×
        </button>
      </li>
    </ul>
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
      />
      <div class="actions">
        <Button
          data-testid="composer-add-attachment"
          :disabled="!capabilities.canSend || sendPending"
          :state="attachmentPending ? 'loading' : 'default'"
          @click="emit('addAttachments')"
        >
          添加图片
        </Button>
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
.composer.is-drop-active {
  outline: 2px solid var(--ctp-blue);
  outline-offset: -2px;
}
.drop-zone {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: grid;
  place-content: center;
  gap: var(--space-1);
  color: var(--ctp-text);
  text-align: center;
  pointer-events: none;
  background: color-mix(in srgb, var(--ctp-mantle) 92%, transparent);
  border: 2px dashed var(--ctp-blue);
}
.drop-zone-icon {
  color: var(--ctp-blue);
  font-size: 28px;
}
.drop-zone span:last-child {
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
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
.attachment-list {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding: 0;
  margin: 0 0 var(--space-2);
  list-style: none;
}
.attachment-chip {
  display: inline-flex;
  max-width: 100%;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  color: var(--ctp-text);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.attachment-name {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.attachment-size {
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.attachment-remove {
  padding: 0 var(--space-1);
  color: var(--ctp-subtext0);
  cursor: pointer;
  background: transparent;
  border: 0;
}
.attachment-remove:focus-visible {
  outline: 2px solid var(--ctp-blue);
  outline-offset: 2px;
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
