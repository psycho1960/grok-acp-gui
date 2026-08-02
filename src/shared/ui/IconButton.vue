<script setup lang="ts">
defineProps<{ label: string; disabled?: boolean }>();
defineEmits<{ click: [event: MouseEvent] }>();
</script>
<template><button class="icon-button" type="button" :aria-label="label" :title="label" :disabled="disabled" @click="$emit('click', $event)"><slot /></button></template>
<style scoped>.icon-button { display: inline-grid; width: var(--control-min-size); height: var(--control-min-size); place-items: center; color: var(--ctp-text); background: transparent; border: 1px solid transparent; border-radius: var(--radius-control); cursor: pointer; }.icon-button:hover { background: var(--ctp-surface0); border-color: var(--ctp-surface1); }.icon-button:disabled { color: var(--ctp-overlay0); cursor: not-allowed; }</style>
