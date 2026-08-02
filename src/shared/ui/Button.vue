<script setup lang="ts">
import { isUnavailable, type ControlState } from "./control-state";
withDefaults(defineProps<{ variant?: "primary" | "secondary" | "danger" | "ghost"; state?: ControlState; disabled?: boolean; type?: "button" | "submit" | "reset" }>(), { variant: "secondary", state: "default", type: "button" });
defineEmits<{ click: [event: MouseEvent] }>();
</script>
<template><button class="ui-button" :class="`is-${variant}`" :data-state="state" :type="type" :disabled="isUnavailable(state, disabled)" :aria-busy="state === 'loading' || undefined" @click="$emit('click', $event)"><span v-if="state === 'loading'" aria-hidden="true">⌛</span><slot /></button></template>
<style scoped>
.ui-button { min-height: var(--button-height); padding: 0 var(--space-3); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); color: var(--ctp-text); background: var(--ctp-surface0); cursor: pointer; transition: background var(--motion-fast), border-color var(--motion-fast); }
.ui-button:hover, .ui-button[data-state="hover"] { background: var(--ctp-surface1); }.ui-button:active, .ui-button[data-state="active"] { border-color: var(--ctp-surface2); }.ui-button[data-state="focus"] { outline:2px solid var(--ctp-mauve); outline-offset:2px; }.is-primary { color: var(--ctp-crust); background: var(--ctp-mauve); border-color: var(--ctp-mauve); }.is-danger { color: var(--ctp-crust); background: var(--ctp-red); border-color: var(--ctp-red); }.is-ghost { background: transparent; }.ui-button[data-state="error"] { border-color: var(--ctp-red); }.ui-button:disabled { color: var(--ctp-overlay0); cursor: not-allowed; opacity: .75; }
</style>
