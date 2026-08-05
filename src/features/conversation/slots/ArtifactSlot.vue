<script setup lang="ts">
import Badge from "../../../shared/ui/Badge.vue";
import type { ArtifactSlotView } from "../types";

defineProps<{ slotData: ArtifactSlotView }>();
</script>

<template>
  <section
    class="artifact-slot surface-card"
    data-testid="artifact-slot"
    aria-label="Artifact 插槽"
  >
    <header>
      <Badge tone="success">Artifact</Badge>
      <span class="name">{{ slotData.displayName }}</span>
    </header>
    <p class="meta">{{ slotData.mimeType }} · {{ slotData.artifactId }}</p>
    <p class="hint">预览与保存由 GAG-010 实现。</p>
  </section>
</template>

<style scoped>
.artifact-slot {
  padding: var(--space-3);
  display: grid;
  gap: var(--space-1);
}
header {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.name {
  font-weight: 600;
}
.meta,
.hint {
  margin: 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
</style>
