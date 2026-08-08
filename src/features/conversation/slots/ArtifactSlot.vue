<script setup lang="ts">
import Badge from "../../../shared/ui/Badge.vue";
import type { ArtifactSlotView } from "../types";

const props = defineProps<{ slotData: ArtifactSlotView }>();
const emit = defineEmits<{ open: [artifactId: string] }>();
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
    <p v-if="slotData.state !== 'ready'" class="hint warning">该 Artifact 已隔离或不可用，不能预览。</p>
    <button v-else type="button" class="open" @click="emit('open', props.slotData.artifactId)">在右侧查看</button>
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
.warning { color: var(--ctp-red); }
.open { justify-self: start; min-height: var(--control-min-size); padding: 0 var(--space-2); color: var(--ctp-text); background: var(--ctp-surface0); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); cursor: pointer; }
</style>
