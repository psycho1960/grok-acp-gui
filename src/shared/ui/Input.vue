<script setup lang="ts">
import { useId } from "vue";
import { isUnavailable, type ControlState } from "./control-state";
withDefaults(defineProps<{ modelValue?: string; label: string; error?: string; disabled?: boolean; placeholder?: string; state?: ControlState }>(), { modelValue: "", error: undefined, placeholder: undefined, state: "default" });
defineEmits<{ "update:modelValue": [value: string] }>();
const errorId = useId();
</script>
<template><label class="field"><span>{{ label }}</span><input :value="modelValue" :placeholder="placeholder" :disabled="isUnavailable(state, disabled)" :data-state="state" :aria-busy="state === 'loading' || undefined" :aria-invalid="Boolean(error) || state === 'error'" :aria-describedby="error ? errorId : undefined" @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)" /><small v-if="error" :id="errorId" role="alert">{{ error }}</small></label></template>
<style scoped>.field { display: grid; gap: var(--space-1); color: var(--ctp-subtext0); font-size: var(--font-small); }.field input { width: 100%; min-height: var(--button-height); padding: 0 var(--space-2); color: var(--ctp-text); background: var(--ctp-surface0); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); }.field input:hover, .field input[data-state="hover"] { background:var(--ctp-surface1); }.field input[data-state="focus"] { outline:2px solid var(--ctp-mauve); outline-offset:2px; }.field input[data-state="active"] { border-color:var(--ctp-surface2); }.field input[aria-invalid="true"] { border-color: var(--ctp-red); }.field input:disabled { color:var(--ctp-overlay0); }.field small { color: var(--ctp-red); }</style>
