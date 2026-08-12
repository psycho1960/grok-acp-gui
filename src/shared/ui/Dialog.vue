<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import IconButton from "./IconButton.vue";
import NamedIcon from "./NamedIcon.vue";
import { focusFirst, keepFocusInside } from "./focus-trap";

const props = defineProps<{ modelValue: boolean; title: string; description?: string }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
const dialog = ref<HTMLElement>();
let restoreFocus: HTMLElement | null = null;

function close(): void {
  emit("update:modelValue", false);
}
function trapFocus(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    close();
    return;
  }
  if (dialog.value) keepFocusInside(dialog.value, event);
}
watch(
  () => props.modelValue,
  async (open) => {
    if (open) {
      restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      await nextTick();
      if (dialog.value) focusFirst(dialog.value);
    } else {
      restoreFocus?.focus();
      restoreFocus = null;
    }
  },
);
</script>

<template>
  <Transition name="dialog-fade">
    <div v-if="modelValue" class="backdrop" @mousedown.self="close">
      <section
        ref="dialog"
        class="dialog"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="'dialog-title-' + title"
        :aria-describedby="description ? 'dialog-description-' + title : undefined"
        @keydown="trapFocus"
      >
        <header>
          <div>
            <h2 :id="'dialog-title-' + title">{{ title }}</h2>
            <p v-if="description" :id="'dialog-description-' + title">{{ description }}</p>
          </div>
          <IconButton label="关闭对话框" @click="close">
            <NamedIcon name="x" :size="16" />
          </IconButton>
        </header>
        <div class="content"><slot /></div>
        <footer><slot name="actions" /></footer>
      </section>
    </div>
  </Transition>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 20;
  display: grid;
  padding: var(--space-4);
  place-items: center;
  background: color-mix(in srgb, var(--ctp-crust) var(--backdrop-alpha), transparent);
}
.dialog {
  width: min(640px, 100%);
  max-height: calc(100vh - 32px);
  overflow: auto;
  color: var(--ctp-text);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-dialog);
  box-shadow: var(--elevation-modal);
}
.dialog-fade-enter-active,
.dialog-fade-leave-active {
  transition: opacity var(--motion-normal) ease;
}
.dialog-fade-enter-active .dialog,
.dialog-fade-leave-active .dialog {
  transition:
    opacity var(--motion-normal) ease,
    transform var(--motion-normal) ease;
}
.dialog-fade-enter-from,
.dialog-fade-leave-to {
  opacity: 0;
}
.dialog-fade-enter-from .dialog,
.dialog-fade-leave-to .dialog {
  opacity: 0;
  transform: scale(0.96);
}
.dialog header,
.dialog footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-4);
}
.dialog header {
  border-bottom: 1px solid var(--ctp-surface0);
}
.dialog footer {
  justify-content: flex-end;
  border-top: 1px solid var(--ctp-surface0);
}
.dialog h2,
.dialog p {
  margin: 0;
}
.dialog h2 {
  font-size: var(--heading-dialog);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
}
.dialog p {
  color: var(--ctp-subtext0);
}
.content {
  padding: var(--space-4);
}
</style>
