<script setup lang="ts">
import { isUnavailable, type ControlState } from "./control-state";
withDefaults(defineProps<{ modelValue?: string; label: string; options: readonly { value: string; label: string }[]; disabled?: boolean; state?: ControlState }>(), { modelValue: "", state: "default" });
defineEmits<{ "update:modelValue": [value: string] }>();
</script>
<template><label class="field"><span>{{ label }}</span><select :value="modelValue" :disabled="isUnavailable(state, disabled)" :data-state="state" :aria-busy="state === 'loading' || undefined" :aria-invalid="state === 'error'" @change="$emit('update:modelValue', ($event.target as HTMLSelectElement).value)"><option v-for="option in options" :key="option.value" :value="option.value">{{ option.label }}</option></select></label></template>
<style scoped>
.field {
  display: grid;
  min-width: 0;
  gap: var(--space-1);
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.field select {
  width: 100%;
  min-width: 0;
  max-width: 100%;
  min-height: var(--button-height);
  padding: 0 var(--space-2);
  color: var(--ctp-text);
  color-scheme: dark;
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.field option {
  color: var(--ctp-text);
  background: var(--ctp-surface0);
}
.field option:checked {
  background: var(--ctp-surface2);
}
.field select:hover,
.field select[data-state="hover"] {
  background: var(--ctp-surface1);
}
.field select[data-state="focus"] {
  outline: 2px solid var(--ctp-mauve);
  outline-offset: 2px;
}
.field select[data-state="active"] {
  border-color: var(--ctp-surface2);
}
.field select[data-state="error"] {
  border-color: var(--ctp-red);
}
.field select:disabled {
  color: var(--ctp-overlay0);
}
</style>
