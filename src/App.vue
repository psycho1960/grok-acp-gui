<script setup lang="ts">
import { onMounted, ref } from "vue";
import { bootstrap, type BootstrapStatus } from "./bridge/desktop-bridge";
import ErrorState from "./shared/ui/ErrorState.vue";
import ShellView from "./app/ShellView.vue";
import UiKitFixture from "./app/UiKitFixture.vue";

const isLoading = ref(true); const startupError = ref<string | null>(null); const bootstrapStatus = ref<BootstrapStatus | null>(null);
const developmentRoute = import.meta.env.DEV ? window.location.hash : "";
const showUiKit = developmentRoute === "#ui-kit";
const showShellPreview = developmentRoute === "#shell";
onMounted(async () => { if (showUiKit || showShellPreview) { isLoading.value = false; return; } try { bootstrapStatus.value = await bootstrap(); } catch (error) { startupError.value = error instanceof Error ? error.message : String(error); } finally { isLoading.value = false; } });
</script>
<template><UiKitFixture v-if="showUiKit" /><ShellView v-else-if="showShellPreview" /><main v-else-if="isLoading" class="startup"><p role="status">正在启动 Grok ACP GUI…</p></main><main v-else-if="startupError" class="startup"><ErrorState title="启动失败" :detail="startupError" /></main><ShellView v-else :data-version="bootstrapStatus?.version" /></template>
<style scoped>.startup { display:grid; min-height:100vh; padding:var(--space-6); place-items:center; background:var(--ctp-base); }.startup > p { color:var(--ctp-subtext0); }</style>
