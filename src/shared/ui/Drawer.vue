<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import IconButton from "./IconButton.vue";
import NamedIcon from "./NamedIcon.vue";
import { focusFirst, keepFocusInside } from "./focus-trap";

const props = defineProps<{ modelValue: boolean; title: string }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
const drawer = ref<HTMLElement>();
let restoreFocus: HTMLElement | null = null;
let swipeStartX = 0;
let swipeActive = false;

function close(): void {
  emit("update:modelValue", false);
}
function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    close();
    return;
  }
  if (drawer.value) keepFocusInside(drawer.value, event);
}

function onPointerDown(event: PointerEvent): void {
  if (event.pointerType === "mouse" && event.button !== 0) return;
  swipeStartX = event.clientX;
  swipeActive = true;
  (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
}

function onPointerUp(event: PointerEvent): void {
  if (!swipeActive) return;
  swipeActive = false;
  const delta = event.clientX - swipeStartX;
  // Swipe right (away from left-edge content / closing edge panels from the right)
  if (delta > 72) close();
}

watch(
  () => props.modelValue,
  async (open) => {
    if (open) {
      restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      await nextTick();
      if (drawer.value) focusFirst(drawer.value);
    } else {
      restoreFocus?.focus();
      restoreFocus = null;
    }
  },
);
</script>

<template>
  <Transition name="drawer-slide">
    <div v-if="modelValue" class="drawer-layer" data-testid="drawer-layer" @mousedown.self="close">
      <aside
        ref="drawer"
        class="drawer"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        @keydown="onKeydown"
        @pointerdown="onPointerDown"
        @pointerup="onPointerUp"
        @pointercancel="swipeActive = false"
      >
        <header>
          <h2>{{ title }}</h2>
          <IconButton label="关闭抽屉" @click="close">
            <NamedIcon name="x" :size="16" />
          </IconButton>
        </header>
        <div class="drawer-content"><slot /></div>
      </aside>
    </div>
  </Transition>
</template>

<style scoped>
.drawer-layer {
  position: fixed;
  inset: 0;
  z-index: 15;
  display: flex;
  justify-content: flex-end;
  background: color-mix(in srgb, var(--ctp-crust) var(--backdrop-alpha), transparent);
  touch-action: pan-y;
}
.drawer {
  width: min(380px, 100vw);
  height: 100%;
  overflow: auto;
  color: var(--ctp-text);
  background: var(--ctp-mantle);
  border-left: 1px solid var(--ctp-surface1);
  box-shadow: var(--elevation-drawer);
  touch-action: pan-y;
}
.drawer-slide-enter-active,
.drawer-slide-leave-active {
  transition: opacity var(--motion-normal) ease;
}
.drawer-slide-enter-active .drawer,
.drawer-slide-leave-active .drawer {
  transition: transform var(--motion-normal) ease;
}
.drawer-slide-enter-from,
.drawer-slide-leave-to {
  opacity: 0;
}
.drawer-slide-enter-from .drawer,
.drawer-slide-leave-to .drawer {
  transform: translateX(100%);
}
.drawer header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4);
  border-bottom: 1px solid var(--ctp-surface0);
}
.drawer h2 {
  margin: 0;
  font-size: var(--heading-panel);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
}
.drawer-content {
  padding: var(--space-4);
}
</style>
