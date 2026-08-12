<script setup lang="ts">
import NamedIcon from "./NamedIcon.vue";

defineProps<{ title: string; detail: string }>();
</script>

<template>
  <section class="empty-state" role="status">
    <span class="empty-icon" aria-hidden="true">
      <NamedIcon name="info" :size="22" />
    </span>
    <div>
      <h2>{{ title }}</h2>
      <p>{{ detail }}</p>
      <slot />
    </div>
  </section>
</template>

<style scoped>
.empty-state {
  display: flex;
  gap: var(--space-3);
  align-items: flex-start;
  max-width: 560px;
  padding: var(--space-6);
  color: var(--ctp-text);
  background: var(--ctp-mantle);
  border: 1px dashed var(--ctp-surface1);
  border-radius: var(--radius-card);
}
.empty-icon {
  color: var(--ctp-blue);
}
.empty-state h2,
.empty-state p {
  margin: 0;
}
.empty-state h2 {
  font-size: var(--heading-panel);
  line-height: var(--leading-tight);
  font-weight: var(--font-weight-semibold);
}
.empty-state p {
  margin-top: var(--space-1);
  color: var(--ctp-subtext0);
}
</style>
