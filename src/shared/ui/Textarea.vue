<script setup lang="ts">
import { useId } from "vue";
import { isUnavailable, type ControlState } from "./control-state";
withDefaults(defineProps<{ modelValue?: string; label: string; error?: string; disabled?: boolean; placeholder?: string; state?: ControlState }>(), { modelValue: "", error: undefined, placeholder: undefined, state: "default" });
defineEmits<{ "update:modelValue": [value: string] }>();
const errorId = useId();
</script>
<template><label class="field"><span>{{ label }}</span><textarea :value="modelValue" :placeholder="placeholder" :disabled="isUnavailable(state, disabled)" :data-state="state" :aria-busy="state === 'loading' || undefined" :aria-invalid="Boolean(error) || state === 'error'" :aria-describedby="error ? errorId : undefined" @input="$emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)" /><small v-if="error" :id="errorId" role="alert">{{ error }}</small></label></template>
<style scoped>.field { display: grid; gap: var(--space-1); color: var(--ctp-subtext0); font-size: var(--font-small); }.field textarea { min-height: 88px; padding: var(--space-2); color: var(--ctp-text); resize: vertical; background: var(--ctp-surface0); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); }.field textarea:hover, .field textarea[data-state="hover"] { background:var(--ctp-surface1); }.field textarea[data-state="focus"] { outline:2px solid var(--ctp-mauve); outline-offset:2px; }.field textarea[data-state="active"] { border-color:var(--ctp-surface2); }.field textarea[aria-invalid="true"] { border-color: var(--ctp-red); }.field textarea:disabled { color:var(--ctp-overlay0); }.field small { color: var(--ctp-red); }</style>
