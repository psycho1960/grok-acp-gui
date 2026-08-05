<script setup lang="ts">
import { ref } from "vue";
import Button from "../../shared/ui/Button.vue";
import type { ComposerCapabilities } from "./types";

const props = defineProps<{
  modelValue: string;
  capabilities: ComposerCapabilities;
  sendError?: string | null;
  sendPending?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  send: [];
  cancel: [];
}>();

const textarea = ref<HTMLTextAreaElement | null>(null);

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
    if (props.capabilities.canSend && props.modelValue.trim()) {
      emit("send");
    }
  }
}

defineExpose({ textarea, focus: () => textarea.value?.focus() });
</script>

<template>
  <footer class="composer" data-testid="composer" aria-label="消息输入">
    <p v-if="capabilities.disabledReason" class="disabled-reason" role="status">
      {{ capabilities.disabledReason }}
    </p>
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
      />
      <div class="actions">
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
          :disabled="!capabilities.canSend || !modelValue.trim()"
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
  flex-shrink: 0;
  padding: var(--space-3);
  border-top: 1px solid var(--ctp-surface0);
  background: var(--ctp-mantle);
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
