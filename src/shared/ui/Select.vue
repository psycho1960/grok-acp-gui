<script setup lang="ts">
defineProps<{ modelValue?: string; label: string; options: readonly { value: string; label: string }[]; disabled?: boolean }>();
defineEmits<{ "update:modelValue": [value: string] }>();
</script>
<template><label class="field"><span>{{ label }}</span><select :value="modelValue" :disabled="disabled" @change="$emit('update:modelValue', ($event.target as HTMLSelectElement).value)"><option v-for="option in options" :key="option.value" :value="option.value">{{ option.label }}</option></select></label></template>
<style scoped>.field { display:grid; gap:var(--space-1); color:var(--ctp-subtext0); font-size:var(--font-small); }.field select { min-height:var(--button-height); padding:0 var(--space-2); color:var(--ctp-text); background:var(--ctp-surface0); border:1px solid var(--ctp-surface1); border-radius:var(--radius-control); }</style>
