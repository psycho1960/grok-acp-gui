<script setup lang="ts">
import { computed, defineComponent, onBeforeUnmount, onMounted, ref, type PropType, type VNode } from "vue";
import Drawer from "../shared/ui/Drawer.vue";
import IconButton from "../shared/ui/IconButton.vue";
import NamedIcon from "../shared/ui/NamedIcon.vue";
import Tooltip from "../shared/ui/Tooltip.vue";
import { BREAKPOINTS } from "../shared/composables/breakpoints";

export type AppShellProps = {
  left: VNode;
  main: VNode;
  inspector?: VNode;
  inspectorOpen: boolean;
  statusBar?: VNode;
  projectLabel?: string;
  workspaceLabel?: string;
};

const props = defineProps({
  left: { type: Object as PropType<VNode>, required: true },
  main: { type: Object as PropType<VNode>, required: true },
  inspector: { type: Object as PropType<VNode>, required: false, default: undefined },
  inspectorOpen: { type: Boolean, required: true },
  statusBar: { type: Object as PropType<VNode>, required: false, default: undefined },
  projectLabel: { type: String, required: false, default: "Project" },
  workspaceLabel: { type: String, required: false, default: "No workspace selected" },
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
const pageZoomed = ref(false);
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
const columnsClass = computed(() => ({ "has-left-resizer": showLeftResizer.value, "has-inspector": showInspectorPanel.value }));

function clampLeft(value: number): number { return Math.min(360, Math.max(220, Math.round(value))); }
function clampRight(value: number): number { return Math.min(600, Math.max(320, Math.round(value))); }
function updateLayoutMode(): void {
  const wasDrawerLayout = drawerLayout.value;
  const wasCompactLayout = compactLayout.value;
  drawerLayout.value = drawerQuery?.matches ?? false;
  compactLayout.value = compactQuery?.matches ?? false;
  fixedLeftWidth.value = fixedLeftQuery?.matches ?? false;
  pageZoomed.value = (window.visualViewport?.scale ?? 1) >= 1.75;
  if (pageZoomed.value) {
    drawerLayout.value = true;
    compactLayout.value = true;
  }
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
  drawerQuery = window.matchMedia(`(max-width: ${BREAKPOINTS.xl}px), (min-resolution: 1.75dppx)`);
  compactQuery = window.matchMedia(`(max-width: ${BREAKPOINTS.compact}px), (min-resolution: 1.75dppx)`);
  fixedLeftQuery = window.matchMedia(`(max-width: ${BREAKPOINTS.lg}px)`);
  updateLayoutMode();
  drawerQuery.addEventListener("change", updateLayoutMode);
  compactQuery.addEventListener("change", updateLayoutMode);
  fixedLeftQuery.addEventListener("change", updateLayoutMode);
  window.visualViewport?.addEventListener("resize", updateLayoutMode);
});
onBeforeUnmount(() => {
  drawerQuery?.removeEventListener("change", updateLayoutMode);
  compactQuery?.removeEventListener("change", updateLayoutMode);
  fixedLeftQuery?.removeEventListener("change", updateLayoutMode);
  window.visualViewport?.removeEventListener("resize", updateLayoutMode);
});
</script>

<template>
  <section class="app-shell" :style="shellStyle" @pointermove="resize" @pointerup="stopResize" @pointercancel="stopResize">
    <a class="skip-link" href="#main-content" data-testid="skip-to-content">跳到主内容</a>
    <header class="shell-topbar">
      <IconButton v-if="compactLayout" label="打开任务导航" :aria-expanded="navigationOpen" data-testid="open-nav" @click="toggleNavigation">
        <NamedIcon name="menu" :size="18" />
      </IconButton>
      <slot name="topbar">
        <span class="project-name" data-testid="topbar-project" :title="projectLabel">{{ projectLabel }}</span>
        <span class="branch" data-testid="topbar-workspace" :title="workspaceLabel">{{ workspaceLabel }}</span>
        <span class="topbar-spacer" />
      </slot>
      <IconButton v-if="inspector" label="打开检查器" :aria-expanded="drawerLayout ? inspectorDrawerOpen : inspectorOpen" data-testid="open-inspector" @click="toggleInspector">
        <NamedIcon name="panels" :size="18" />
      </IconButton>
    </header>
    <div class="shell-columns" :class="columnsClass">
      <aside v-if="showLeftPanel" class="shell-left" aria-label="任务导航"><RenderVNode :node="left" /></aside>
      <Tooltip v-if="showLeftResizer" text="← → 调整左侧栏宽度">
        <div
          class="resizer left-resizer"
          role="separator"
          aria-orientation="vertical"
          aria-label="调整左侧栏宽度。使用左右方向键调整。"
          :aria-valuemin="220"
          :aria-valuemax="360"
          :aria-valuenow="leftWidth"
          tabindex="0"
          data-testid="left-resizer"
          @pointerdown.prevent="startResize('left')"
          @keydown="resizeLeftWithKeyboard"
        />
      </Tooltip>
      <main id="main-content" class="shell-main" aria-label="主内容" tabindex="-1"><RenderVNode :node="main" /></main>
      <Tooltip v-if="showInspectorPanel" text="← → 调整检查器宽度">
        <div
          class="resizer right-resizer"
          role="separator"
          aria-orientation="vertical"
          aria-label="调整检查器宽度。使用左右方向键调整。"
          :aria-valuemin="320"
          :aria-valuemax="600"
          :aria-valuenow="rightWidth"
          tabindex="0"
          data-testid="right-resizer"
          @pointerdown.prevent="startResize('right')"
          @keydown="resizeRightWithKeyboard"
        />
      </Tooltip>
      <aside v-if="showInspectorPanel" class="shell-inspector" aria-label="检查器"><RenderVNode :node="inspector!" /></aside>
    </div>
    <footer class="shell-statusbar"><slot name="statusbar"><RenderVNode v-if="statusBar" :node="statusBar" /></slot></footer>
    <Drawer v-if="compactLayout" :model-value="navigationOpen" title="任务导航" @update:model-value="navigationOpen = $event"><RenderVNode :node="left" /></Drawer>
    <Drawer v-if="inspector && drawerLayout" :model-value="inspectorDrawerOpen" title="检查器" @update:model-value="inspectorDrawerOpen = $event"><RenderVNode :node="inspector" /></Drawer>
  </section>
</template>

<style scoped>
.app-shell { display:grid; grid-template-rows:48px minmax(0, 1fr) 28px; height:100vh; min-height:680px; overflow:hidden; background:var(--ctp-base); }
.skip-link {
  position: absolute;
  top: 0;
  left: var(--space-3);
  z-index: 50;
  padding: var(--space-2) var(--space-3);
  color: var(--ctp-crust);
  background: var(--ctp-mauve);
  border-radius: 0 0 var(--radius-control) var(--radius-control);
  transform: translateY(-120%);
  transition: transform var(--motion-fast) ease;
}
.skip-link:focus {
  transform: translateY(0);
  outline: 2px solid var(--ctp-text);
  outline-offset: 2px;
}
.shell-topbar, .shell-statusbar { display:flex; align-items:center; gap:var(--space-3); padding:0 var(--space-4); background:var(--ctp-crust); border-color:var(--ctp-surface0); }
.shell-topbar { border-bottom:1px solid var(--ctp-surface0); }.shell-statusbar { color:var(--ctp-subtext0); border-top:1px solid var(--ctp-surface0); font-size:var(--font-small); }
.project-name, .branch { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.project-name { max-width:220px; color:var(--ctp-text); font-weight:var(--font-weight-semibold); }.branch { max-width:360px; color:var(--ctp-subtext0); font-size:var(--font-small); }.topbar-spacer { flex:1; }
.shell-columns { display:grid; grid-template-columns:var(--left-width) minmax(520px, 1fr); min-width:0; }
.shell-columns.has-left-resizer { grid-template-columns:var(--left-width) 4px minmax(520px, 1fr); }
.shell-columns.has-inspector { grid-template-columns:var(--left-width) minmax(520px, 1fr) 4px var(--right-width); }
.shell-columns.has-left-resizer.has-inspector { grid-template-columns:var(--left-width) 4px minmax(520px, 1fr) 4px var(--right-width); }
.shell-left, .shell-inspector { min-width:0; overflow:auto; padding:var(--space-4); background:var(--ctp-mantle); }.shell-left { border-right:1px solid var(--ctp-surface0); }.shell-inspector { border-left:1px solid var(--ctp-surface0); }
.shell-main { min-width:0; overflow:auto; padding:var(--space-6); }
.shell-main:focus { outline: none; }
.shell-main:focus-visible { outline: 2px solid var(--ctp-mauve); outline-offset: -2px; }
.resizer { width: 100%; height: 100%; min-height: 100%; cursor:col-resize; background:var(--ctp-surface0); }
.resizer:hover, .resizer:focus-visible { background:var(--ctp-mauve); outline:none; }
/* Tooltip wraps resizer so the shell grid still gets a 4px column. */
:deep(.tooltip) { display: block; width: 4px; height: 100%; min-height: 100%; align-self: stretch; }
@media (max-width: 1080px) { .shell-main { padding:var(--space-4); } /* BREAKPOINTS.lg */ }
@media (max-width: 1023px), (min-resolution: 1.75dppx) { .app-shell { min-height:0; }.shell-columns, .shell-columns.has-left-resizer, .shell-columns.has-inspector, .shell-columns.has-left-resizer.has-inspector { grid-template-columns:minmax(0, 1fr); }.shell-main { min-width:0; }.shell-topbar { min-height:48px; }.shell-statusbar { min-height:28px; } /* BREAKPOINTS.compact */ }
/* High-DPI / fractional scale: keep control hit areas readable at 150%+ */
@media (min-resolution: 1.5dppx) {
  .shell-topbar, .shell-statusbar { min-height: 48px; }
  .resizer { width: 5px; }
}
@media print {
  .skip-link,
  .shell-topbar,
  .shell-statusbar,
  .shell-left,
  .shell-inspector,
  .resizer,
  :deep(.composer),
  :deep(.jump-bottom) {
    display: none !important;
  }
  .app-shell {
    display: block;
    height: auto;
    min-height: 0;
    overflow: visible;
    background: white;
    color: black;
  }
  .shell-columns,
  .shell-columns.has-left-resizer,
  .shell-columns.has-inspector,
  .shell-columns.has-left-resizer.has-inspector {
    display: block;
  }
  .shell-main {
    overflow: visible;
    padding: 0;
    color: black;
    background: white;
  }
  :deep(pre),
  :deep(code) {
    color: black !important;
    background: #f4f4f5 !important;
    border: 1px solid #ccc;
  }
}
</style>
