<script setup lang="ts">
defineProps<{ status: "running" | "waiting" | "success" | "error" | "interrupted"; label: string }>();
const glyph = { running: "◌", waiting: "◈", success: "✓", error: "!", interrupted: "⌁" } as const;
</script>
<template><span class="status" :class="`is-${status}`" role="img" :aria-label="label"><span aria-hidden="true">{{ glyph[status] }}</span><span class="label">{{ label }}</span></span></template>
<style scoped>.status { display:inline-flex; gap:6px; align-items:center; font-size:var(--font-small); }.is-running { color:var(--ctp-blue); }.is-waiting { color:var(--ctp-yellow); }.is-success { color:var(--ctp-green); }.is-error { color:var(--ctp-red); }.is-interrupted { color:var(--ctp-peach); }.is-running > span:first-child { animation:spin 1s linear infinite; }@keyframes spin { to { transform:rotate(1turn); } }</style>
