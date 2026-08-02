<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import IconButton from "./IconButton.vue";

const props = defineProps<{ modelValue: boolean; title: string; description?: string }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
const dialog = ref<HTMLElement>();
let restoreFocus: HTMLElement | null = null;

function close(): void { emit("update:modelValue", false); }
function trapFocus(event: KeyboardEvent): void {
  if (event.key === "Escape") { event.preventDefault(); close(); return; }
  if (event.key !== "Tab" || !dialog.value) return;
  const focusable = [...dialog.value.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])')];
  if (!focusable.length) return;
  const first = focusable[0]; const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
}
watch(() => props.modelValue, async (open) => {
  if (open) { restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null; await nextTick(); dialog.value?.querySelector<HTMLElement>("button, [tabindex]")?.focus(); }
  else { restoreFocus?.focus(); restoreFocus = null; }
});
</script>
<template><div v-if="modelValue" class="backdrop" @mousedown.self="close"><section ref="dialog" class="dialog" role="dialog" aria-modal="true" :aria-labelledby="'dialog-title-' + title" :aria-describedby="description ? 'dialog-description-' + title : undefined" @keydown="trapFocus"><header><div><h2 :id="'dialog-title-' + title">{{ title }}</h2><p v-if="description" :id="'dialog-description-' + title">{{ description }}</p></div><IconButton label="关闭对话框" @click="close">×</IconButton></header><div class="content"><slot /></div><footer><slot name="actions" /></footer></section></div></template>
<style scoped>.backdrop { position:fixed; inset:0; z-index:20; display:grid; padding:var(--space-4); place-items:center; background:color-mix(in srgb, var(--ctp-crust) 78%, transparent); }.dialog { width:min(640px, 100%); max-height:calc(100vh - 32px); overflow:auto; color:var(--ctp-text); background:var(--ctp-mantle); border:1px solid var(--ctp-surface1); border-radius:var(--radius-dialog); box-shadow:0 24px 64px color-mix(in srgb, var(--ctp-crust) 60%, transparent); }.dialog header, .dialog footer { display:flex; align-items:center; justify-content:space-between; gap:var(--space-3); padding:var(--space-4); }.dialog header { border-bottom:1px solid var(--ctp-surface0); }.dialog footer { justify-content:flex-end; border-top:1px solid var(--ctp-surface0); }.dialog h2, .dialog p { margin:0; }.dialog h2 { font-size:20px; line-height:28px; }.dialog p { color:var(--ctp-subtext0); }.content { padding:var(--space-4); }</style>
