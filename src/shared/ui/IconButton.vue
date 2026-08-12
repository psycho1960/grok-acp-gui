<script setup lang="ts">
import { isUnavailable, type ControlState } from "./control-state";

withDefaults(
  defineProps<{ label: string; disabled?: boolean; state?: ControlState }>(),
  { state: "default" },
);
defineEmits<{ click: [event: MouseEvent] }>();
</script>

<template>
  <button
    class="icon-button"
    type="button"
    :aria-label="label"
    :title="label"
    :data-state="state"
    :disabled="isUnavailable(state, disabled)"
    :aria-busy="state === 'loading' || undefined"
    @click="$emit('click', $event)"
  >
    <slot />
  </button>
</template>

<style scoped>
.icon-button {
  display: inline-grid;
  width: var(--control-min-size);
  height: var(--control-min-size);
  place-items: center;
  color: var(--ctp-text);
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-control);
  cursor: pointer;
  transition:
    background var(--motion-fast),
    border-color var(--motion-fast),
    transform var(--motion-fast);
}
.icon-button:hover,
.icon-button[data-state="hover"] {
  background: var(--ctp-surface0);
  border-color: var(--ctp-surface1);
}
.icon-button[data-state="focus"] {
  outline: 2px solid var(--ctp-mauve);
  outline-offset: 2px;
}
.icon-button:active,
.icon-button[data-state="active"] {
  background: var(--ctp-surface1);
  transform: scale(0.94);
}
.icon-button[data-state="error"] {
  border-color: var(--ctp-red);
}
.icon-button:disabled {
  color: var(--ctp-overlay0);
  cursor: not-allowed;
  transform: none;
}
</style>
