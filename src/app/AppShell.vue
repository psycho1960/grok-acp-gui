<script setup lang="ts">
import { computed, defineComponent, onBeforeUnmount, onMounted, ref, type PropType, type VNode } from "vue";
import Drawer from "../shared/ui/Drawer.vue";
import IconButton from "../shared/ui/IconButton.vue";

export type AppShellProps = {
  left: VNode;
  main: VNode;
  inspector?: VNode;
  inspectorOpen: boolean;
  statusBar?: VNode;
};

const props = defineProps({
  left: { type: Object as PropType<VNode>, required: true },
  main: { type: Object as PropType<VNode>, required: true },
  inspector: { type: Object as PropType<VNode>, required: false, default: undefined },
  inspectorOpen: { type: Boolean, required: true },
  statusBar: { type: Object as PropType<VNode>, required: false, default: undefined },
});
const emit = defineEmits<{ "update:inspectorOpen": [value: boolean] }>();
const RenderVNode = defineComponent({
  props: { node: { type: Object as PropType<VNode>, required: true } },
  setup(props) { return () => props.node; },
});
const leftWidth = ref(260);
const isResizing = ref(false);
const narrowLayout = ref(false);
const drawerOpen = ref(false);
let viewportQuery: MediaQueryList | undefined;
function updateLayoutMode(): void {
  const wasNarrow = narrowLayout.value;
  narrowLayout.value = viewportQuery?.matches ?? false;
  if (narrowLayout.value && !wasNarrow) drawerOpen.value = false;
}
onMounted(() => {
  viewportQuery = window.matchMedia("(max-width: 1200px)");
  updateLayoutMode();
  viewportQuery.addEventListener("change", updateLayoutMode);
});
onBeforeUnmount(() => viewportQuery?.removeEventListener("change", updateLayoutMode));
const shellStyle = computed(() => ({ "--left-width": `${leftWidth.value}px` }));
function clampLeft(value: number): number { return Math.min(360, Math.max(220, Math.round(value))); }
function startResize(): void { isResizing.value = true; }
function resize(event: PointerEvent): void { if (isResizing.value) leftWidth.value = clampLeft(event.clientX); }
function stopResize(): void { isResizing.value = false; }
function resizeWithKeyboard(event: KeyboardEvent): void { if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); leftWidth.value = clampLeft(leftWidth.value + (event.key === "ArrowRight" ? 12 : -12)); } }
function toggleInspector(): void { if (narrowLayout.value) drawerOpen.value = !drawerOpen.value; else emit("update:inspectorOpen", !props.inspectorOpen); }
</script>
<template>
  <section class="app-shell" :style="shellStyle" @pointermove="resize" @pointerup="stopResize" @pointercancel="stopResize">
    <header class="shell-topbar"><slot name="topbar"><span class="project-name">Project</span><span class="branch">No workspace selected</span><span class="topbar-spacer" /><IconButton v-if="inspector" label="打开 Inspector" :aria-pressed="narrowLayout ? drawerOpen : inspectorOpen" @click="toggleInspector">☷</IconButton></slot></header>
    <div class="shell-columns">
      <aside class="shell-left" aria-label="任务导航"><RenderVNode :node="left" /></aside>
      <div class="resizer" role="separator" aria-orientation="vertical" aria-label="调整左侧栏宽度" :aria-valuemin="220" :aria-valuemax="360" :aria-valuenow="leftWidth" tabindex="0" @pointerdown.prevent="startResize" @keydown="resizeWithKeyboard" />
      <main class="shell-main" aria-label="主内容"><RenderVNode :node="main" /></main>
      <aside v-if="inspector && inspectorOpen" class="shell-inspector" aria-label="Inspector"><RenderVNode :node="inspector" /></aside>
    </div>
    <footer class="shell-statusbar"><slot name="statusbar"><RenderVNode v-if="statusBar" :node="statusBar" /></slot></footer>
    <Drawer v-if="inspector" :model-value="drawerOpen" title="Inspector" @update:model-value="drawerOpen = $event"><RenderVNode :node="inspector" /></Drawer>
  </section>
</template>
<style scoped>
.app-shell { display:grid; grid-template-rows:48px minmax(0, 1fr) 28px; height:100vh; min-height:680px; overflow:hidden; background:var(--ctp-base); }.shell-topbar, .shell-statusbar { display:flex; align-items:center; gap:var(--space-3); padding:0 var(--space-4); background:var(--ctp-crust); border-color:var(--ctp-surface0); }.shell-topbar { border-bottom:1px solid var(--ctp-surface0); }.shell-statusbar { color:var(--ctp-subtext0); border-top:1px solid var(--ctp-surface0); font-size:var(--font-small); }.project-name { color:var(--ctp-text); font-weight:650; }.branch { color:var(--ctp-subtext0); font-size:var(--font-small); }.topbar-spacer { flex:1; }.shell-columns { display:grid; grid-template-columns:var(--left-width) 4px minmax(520px, 1fr) minmax(320px, 380px); min-width:0; }.shell-left, .shell-inspector { min-width:0; overflow:auto; padding:var(--space-4); background:var(--ctp-mantle); }.shell-left { border-right:1px solid var(--ctp-surface0); }.shell-inspector { border-left:1px solid var(--ctp-surface0); }.shell-main { min-width:0; overflow:auto; padding:var(--space-6); }.resizer { cursor:col-resize; background:var(--ctp-surface0); }.resizer:hover, .resizer:focus-visible { background:var(--ctp-mauve); outline:none; }
@media (max-width: 1200px) { .shell-columns { grid-template-columns:var(--left-width) 4px minmax(520px, 1fr); }.shell-inspector { display:none; }.app-shell > .drawer-layer { display:flex; } }
@media (min-width: 1201px) { .app-shell > .drawer-layer { display:none; } }
@media (max-width: 1080px) { .app-shell { --left-width:220px !important; }.shell-main { padding:var(--space-4); } }
@media (max-width: 1023px), (min-resolution: 1.75dppx) { .app-shell { min-height:0; }.shell-columns { grid-template-columns:minmax(0, 1fr); }.shell-left, .resizer { display:none; }.shell-main { min-width:0; }.shell-topbar { min-height:48px; }.shell-statusbar { min-height:28px; } }
</style>
