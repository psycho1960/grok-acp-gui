<script setup lang="ts">
import {
  computed,
  type ComponentPublicInstance,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";

const props = withDefaults(
  defineProps<{
    modelValue?: string;
    label: string;
    options: readonly { value: string; label: string }[];
    disabled?: boolean;
  }>(),
  { modelValue: "", disabled: false },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const root = ref<HTMLElement | null>(null);
const optionButtons = ref<HTMLButtonElement[]>([]);
const open = ref(false);

const selectedIndex = computed(() =>
  Math.max(
    0,
    props.options.findIndex((option) => option.value === props.modelValue),
  ),
);
const selectedLabel = computed(
  () =>
    props.options.find((option) => option.value === props.modelValue)?.label ??
    props.label,
);

function setOptionRef(element: Element | ComponentPublicInstance | null): void {
  if (element instanceof HTMLButtonElement) optionButtons.value.push(element);
}

async function openMenu(): Promise<void> {
  if (props.disabled) return;
  optionButtons.value = [];
  open.value = true;
  await nextTick();
  optionButtons.value[selectedIndex.value]?.focus();
}

function closeMenu(restoreFocus = false): void {
  open.value = false;
  if (restoreFocus) {
    (root.value?.querySelector(".select-trigger") as HTMLButtonElement | null)?.focus();
  }
}

function toggleMenu(): void {
  if (open.value) closeMenu();
  else void openMenu();
}

function choose(value: string): void {
  emit("update:modelValue", value);
  closeMenu(true);
}

function focusRelative(offset: number): void {
  const buttons = optionButtons.value;
  if (buttons.length === 0) return;
  const current = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
  buttons[(current + offset + buttons.length) % buttons.length]?.focus();
}

function onOptionKeydown(event: KeyboardEvent, value: string): void {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    focusRelative(1);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    focusRelative(-1);
  } else if (event.key === "Home") {
    event.preventDefault();
    optionButtons.value[0]?.focus();
  } else if (event.key === "End") {
    event.preventDefault();
    optionButtons.value[optionButtons.value.length - 1]?.focus();
  } else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    choose(value);
  } else if (event.key === "Escape") {
    event.preventDefault();
    closeMenu(true);
  }
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (open.value && !root.value?.contains(event.target as Node)) closeMenu();
}

watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) closeMenu();
  },
);

onMounted(() => document.addEventListener("pointerdown", onDocumentPointerDown));
onBeforeUnmount(() =>
  document.removeEventListener("pointerdown", onDocumentPointerDown),
);
</script>

<template>
  <div ref="root" class="header-select">
    <span class="select-label">{{ label }}</span>
    <button
      type="button"
      class="select-trigger"
      :disabled="disabled"
      :aria-label="label"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :data-selected-value="modelValue"
      data-testid="header-select-trigger"
      @click="toggleMenu"
      @keydown.down.prevent="openMenu"
      @keydown.up.prevent="openMenu"
    >
      <span class="trigger-label">{{ selectedLabel }}</span>
      <slot name="indicator" />
    </button>
    <ul
      v-if="open"
      class="select-menu"
      role="listbox"
      :aria-label="label"
      data-testid="header-select-menu"
    >
      <li v-for="option in options" :key="option.value">
        <button
          :ref="setOptionRef"
          type="button"
          class="select-option"
          :class="{ selected: option.value === modelValue }"
          role="option"
          :aria-selected="option.value === modelValue"
          :data-value="option.value"
          data-testid="header-select-option"
          @click="choose(option.value)"
          @keydown="onOptionKeydown($event, option.value)"
        >
          {{ option.label }}
        </button>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.header-select {
  position: relative;
  min-width: 0;
}
.select-label {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
.select-trigger {
  display: flex;
  gap: var(--space-1);
  align-items: center;
  justify-content: space-between;
  width: 100%;
  min-width: 0;
  min-height: 28px;
  padding: 0;
  overflow: hidden;
  color: inherit;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  background: transparent;
  border: 0;
  cursor: pointer;
}
.trigger-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.select-trigger:focus-visible {
  outline: 2px solid var(--ctp-mauve);
  outline-offset: 2px;
  border-radius: var(--radius-control);
}
.select-trigger:disabled {
  color: var(--ctp-overlay0);
  cursor: default;
}
.select-menu {
  position: absolute;
  top: calc(100% + var(--space-1));
  right: 0;
  z-index: 40;
  display: grid;
  min-width: max(100%, 180px);
  max-width: min(320px, calc(100vw - 32px));
  max-height: min(320px, calc(100vh - 160px));
  margin: 0;
  padding: var(--space-1);
  overflow: auto;
  list-style: none;
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  box-shadow: var(--shadow-md);
}
.select-option {
  width: 100%;
  min-height: var(--control-min-size);
  padding: 0 var(--space-2);
  color: var(--ctp-text);
  text-align: left;
  white-space: nowrap;
  background: transparent;
  border: 0;
  border-radius: var(--radius-control);
  cursor: pointer;
}
.select-option:hover,
.select-option:focus-visible {
  background: var(--overlay-hover);
  outline: none;
}
.select-option.selected {
  background: var(--overlay-menu-active);
}
</style>
