<script setup lang="ts">
import { watch } from "vue";
import IconButton from "./IconButton.vue";
const props = defineProps<{ modelValue: boolean; title: string }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
function close(): void { emit("update:modelValue", false); }
function onKeydown(event: KeyboardEvent): void { if (event.key === "Escape") close(); }
watch(() => props.modelValue, (open) => { if (open) requestAnimationFrame(() => document.querySelector<HTMLElement>(".drawer button")?.focus()); });
</script>
<template><div v-if="modelValue" class="drawer-layer" @mousedown.self="close"><aside class="drawer" role="dialog" aria-modal="true" :aria-label="title" @keydown="onKeydown"><header><h2>{{ title }}</h2><IconButton label="关闭抽屉" @click="close">×</IconButton></header><div class="drawer-content"><slot /></div></aside></div></template>
<style scoped>.drawer-layer { position:fixed; inset:0; z-index:15; display:flex; justify-content:flex-end; background:color-mix(in srgb, var(--ctp-crust) 68%, transparent); }.drawer { width:min(380px, 100vw); height:100%; overflow:auto; color:var(--ctp-text); background:var(--ctp-mantle); border-left:1px solid var(--ctp-surface1); }.drawer header { display:flex; align-items:center; justify-content:space-between; padding:var(--space-4); border-bottom:1px solid var(--ctp-surface0); }.drawer h2 { margin:0; font-size:16px; }.drawer-content { padding:var(--space-4); }</style>
