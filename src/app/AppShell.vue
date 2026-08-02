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
  setup(renderProps) { return () => renderProps.node; },
});

const leftWidth = ref(260);
const rightWidth = ref(380);
const dragging = ref<"left" | "right" | null>(null);
const drawerLayout = ref(false);
const compactLayout = ref(false);
const fixedLeftWidth = ref(false);
const navigationOpen = ref(false);
const inspectorDrawerOpen = ref(false);
let drawerQuery: MediaQueryList | undefined;
let compactQuery: MediaQueryList | undefined;
let fixedLeftQuery: MediaQueryList | undefined;

const shellStyle = computed(() => ({
  "--left-width": `${leftWidth.value}px`,
  "--right-width": `${rightWidth.value}px`,
}));
const showLeftPanel = computed(() => !compactLayout.value);
const showLeftResizer = computed(() => showLeftPanel.value && !fixedLeftWidth.value);
const showInspectorPanel = computed(() => Boolean(props.inspector && props.inspectorOpen && !drawerLayout.value));
const columnsClass = computed(() => ({ "has-inspector": showInspectorPanel.value }));

function clampLeft(value: number): number { return Math.min(360, Math.max(220, Math.round(value))); }
function clampRight(value: number): number { return Math.min(600, Math.max(320, Math.round(value))); }
function updateLayoutMode(): void {
  const wasDrawerLayout = drawerLayout.value;
  const wasCompactLayout = compactLayout.value;
  drawerLayout.value = drawerQuery?.matches ?? false;
  compactLayout.value = compactQuery?.matches ?? false;
  fixedLeftWidth.value = fixedLeftQuery?.matches ?? false;
  if (fixedLeftWidth.value) leftWidth.value = 220;
  if (drawerLayout.value && !wasDrawerLayout) inspectorDrawerOpen.value = false;
  if (compactLayout.value && !wasCompactLayout) navigationOpen.value = false;
}
function startResize(side: "left" | "right"): void { dragging.value = side; }
function resize(event: PointerEvent): void {
  if (dragging.value === "left") leftWidth.value = clampLeft(event.clientX);
  if (dragging.value === "right") rightWidth.value = clampRight(window.innerWidth - event.clientX);
}
function stopResize(): void { dragging.value = null; }
function resizeLeftWithKeyboard(event: KeyboardEvent): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  leftWidth.value = clampLeft(leftWidth.value + (event.key === "ArrowRight" ? 12 : -12));
}
function resizeRightWithKeyboard(event: KeyboardEvent): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  rightWidth.value = clampRight(rightWidth.value + (event.key === "ArrowLeft" ? 12 : -12));
}
function toggleInspector(): void {
  if (drawerLayout.value) inspectorDrawerOpen.value = !inspectorDrawerOpen.value;
  else emit("update:inspectorOpen", !props.inspectorOpen);
}
function toggleNavigation(): void { navigationOpen.value = !navigationOpen.value; }

onMounted(() => {
  drawerQuery = window.matchMedia("(max-width: 1200px), (min-resolution: 1.75dppx)");
  compactQuery = window.matchMedia("(max-width: 1023px), (min-resolution: 1.75dppx)");
  fixedLeftQuery = window.matchMedia("(max-width: 1080px)");
  updateLayoutMode();
  drawerQuery.addEventListener("change", updateLayoutMode);
  compactQuery.addEventListener("change", updateLayoutMode);
  fixedLeftQuery.addEventListener("change", updateLayoutMode);
});
onBeforeUnmount(() => {
  drawerQuery?.removeEventListener("change", updateLayoutMode);
  compactQuery?.removeEventListener("change", updateLayoutMode);
  fixedLeftQuery?.removeEventListener("change", updateLayoutMode);
});
</script>

<template>
  <section class="app-shell" :style="shellStyle" @pointermove="resize" @pointerup="stopResize" @pointercancel="stopResize">
    <header class="shell-topbar">
      <slot name="topbar">
        <IconButton v-if="compactLayout" label="打开任务导航" :aria-expanded="navigationOpen" @click="toggleNavigation">☰</IconButton>
        <span class="project-name">Project</span>
        <span class="branch">No workspace selected</span>
        <span class="topbar-spacer" />
        <IconButton v-if="inspector" label="打开 Inspector" :aria-expanded="drawerLayout ? inspectorDrawerOpen : inspectorOpen" @click="toggleInspector">☷</IconButton>
      </slot>
    </header>
    <div class="shell-columns" :class="columnsClass">
      <aside v-if="showLeftPanel" class="shell-left" aria-label="任务导航"><RenderVNode :node="left" /></aside>
      <div v-if="showLeftResizer" class="resizer left-resizer" role="separator" aria-orientation="vertical" aria-label="调整左侧栏宽度" :aria-valuemin="220" :aria-valuemax="360" :aria-valuenow="leftWidth" tabindex="0" @pointerdown.prevent="startResize('left')" @keydown="resizeLeftWithKeyboard" />
      <main class="shell-main" aria-label="主内容"><RenderVNode :node="main" /></main>
      <div v-if="showInspectorPanel" class="resizer right-resizer" role="separator" aria-orientation="vertical" aria-label="调整 Inspector 宽度" :aria-valuemin="320" :aria-valuemax="600" :aria-valuenow="rightWidth" tabindex="0" @pointerdown.prevent="startResize('right')" @keydown="resizeRightWithKeyboard" />
      <aside v-if="showInspectorPanel" class="shell-inspector" aria-label="Inspector"><RenderVNode :node="inspector!" /></aside>
    </div>
    <footer class="shell-statusbar"><slot name="statusbar"><RenderVNode v-if="statusBar" :node="statusBar" /></slot></footer>
    <Drawer v-if="compactLayout" :model-value="navigationOpen" title="任务导航" @update:model-value="navigationOpen = $event"><RenderVNode :node="left" /></Drawer>
    <Drawer v-if="inspector && drawerLayout" :model-value="inspectorDrawerOpen" title="Inspector" @update:model-value="inspectorDrawerOpen = $event"><RenderVNode :node="inspector" /></Drawer>
  </section>
</template>

<style scoped>
.app-shell { display:grid; grid-template-rows:48px minmax(0, 1fr) 28px; height:100vh; min-height:680px; overflow:hidden; background:var(--ctp-base); }
.shell-topbar, .shell-statusbar { display:flex; align-items:center; gap:var(--space-3); padding:0 var(--space-4); background:var(--ctp-crust); border-color:var(--ctp-surface0); }
.shell-topbar { border-bottom:1px solid var(--ctp-surface0); }.shell-statusbar { color:var(--ctp-subtext0); border-top:1px solid var(--ctp-surface0); font-size:var(--font-small); }
.project-name { color:var(--ctp-text); font-weight:650; }.branch { color:var(--ctp-subtext0); font-size:var(--font-small); }.topbar-spacer { flex:1; }
.shell-columns { display:grid; grid-template-columns:var(--left-width) 4px minmax(520px, 1fr); min-width:0; }
.shell-columns.has-inspector { grid-template-columns:var(--left-width) 4px minmax(520px, 1fr) 4px var(--right-width); }
.shell-left, .shell-inspector { min-width:0; overflow:auto; padding:var(--space-4); background:var(--ctp-mantle); }.shell-left { border-right:1px solid var(--ctp-surface0); }.shell-inspector { border-left:1px solid var(--ctp-surface0); }
.shell-main { min-width:0; overflow:auto; padding:var(--space-6); }.resizer { cursor:col-resize; background:var(--ctp-surface0); }.resizer:hover, .resizer:focus-visible { background:var(--ctp-mauve); outline:none; }
@media (max-width: 1200px), (min-resolution: 1.75dppx) { .shell-columns, .shell-columns.has-inspector { grid-template-columns:var(--left-width) 4px minmax(520px, 1fr); } }
@media (max-width: 1080px) { .shell-main { padding:var(--space-4); } }
@media (max-width: 1023px), (min-resolution: 1.75dppx) { .app-shell { min-height:0; }.shell-columns, .shell-columns.has-inspector { grid-template-columns:minmax(0, 1fr); }.shell-main { min-width:0; }.shell-topbar { min-height:48px; }.shell-statusbar { min-height:28px; } }
</style>
