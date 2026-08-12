<script setup lang="ts">
import { toast, useToastState } from "./toast";
import IconButton from "./IconButton.vue";
import NamedIcon from "./NamedIcon.vue";

const state = useToastState();

function iconFor(tone: string): "check" | "alert" | "info" | "activity" {
  if (tone === "success") return "check";
  if (tone === "warning" || tone === "error") return "alert";
  if (tone === "info") return "info";
  return "activity";
}
</script>

<template>
  <div class="toast-host" aria-live="polite" aria-relevant="additions text" data-testid="toast-host">
    <TransitionGroup name="toast">
      <div
        v-for="item in state.items"
        :key="item.id"
        class="toast"
        :class="`tone-${item.tone}`"
        role="status"
        :data-testid="`toast-${item.tone}`"
        :data-toast-id="item.id"
      >
        <span class="toast-icon" aria-hidden="true">
          <NamedIcon :name="iconFor(item.tone)" :size="16" />
        </span>
        <div class="toast-body">
          <p class="toast-title">{{ item.title }}</p>
          <p v-if="item.description" class="toast-desc">{{ item.description }}</p>
        </div>
        <IconButton :label="`关闭通知：${item.title}`" @click="toast.dismiss(item.id)">
          <NamedIcon name="x" :size="14" />
        </IconButton>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-host {
  position: fixed;
  top: calc(48px + var(--space-3));
  right: var(--space-4);
  z-index: 40;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  width: min(360px, calc(100vw - 32px));
  pointer-events: none;
}
.toast {
  pointer-events: auto;
  display: grid;
  grid-template-columns: auto 1fr auto;
  gap: var(--space-2);
  align-items: start;
  padding: var(--space-3);
  color: var(--ctp-text);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-card);
  box-shadow: var(--elevation-menu);
}
.toast-icon {
  display: grid;
  width: 20px;
  height: 20px;
  place-items: center;
  margin-top: 1px;
}
.toast-body {
  min-width: 0;
}
.toast-title,
.toast-desc {
  margin: 0;
}
.toast-title {
  font-size: var(--text-sm);
  font-weight: var(--font-weight-semibold);
  line-height: var(--leading-tight);
}
.toast-desc {
  margin-top: 2px;
  color: var(--ctp-subtext0);
  font-size: var(--text-sm);
  line-height: var(--leading-normal);
}
.tone-success {
  border-color: var(--border-tone-success);
}
.tone-success .toast-icon {
  color: var(--ctp-green);
}
.tone-warning {
  border-color: var(--border-tone-warning);
}
.tone-warning .toast-icon {
  color: var(--ctp-yellow);
}
.tone-error {
  border-color: var(--border-tone-danger);
}
.tone-error .toast-icon {
  color: var(--ctp-red);
}
.tone-info {
  border-color: var(--border-tone-info);
}
.tone-info .toast-icon {
  color: var(--ctp-blue);
}
.toast-enter-active,
.toast-leave-active {
  transition:
    opacity var(--motion-normal) ease,
    transform var(--motion-normal) ease;
}
.toast-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}
.toast-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
.toast-move {
  transition: transform var(--motion-normal) ease;
}
</style>
