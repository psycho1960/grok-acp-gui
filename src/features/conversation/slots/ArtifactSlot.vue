<script setup lang="ts">
import type { ArtifactSlotView } from "../types";

const props = defineProps<{ slotData: ArtifactSlotView }>();
const emit = defineEmits<{ open: [artifactId: string] }>();
</script>

<template>
  <button
    type="button"
    class="artifact-chip"
    data-testid="artifact-chip"
    :disabled="slotData.state !== 'ready'"
    :aria-label="slotData.displayName"
    @click="emit('open', props.slotData.artifactId)"
  >
    <span class="name">{{ slotData.displayName }}</span>
    <span v-if="slotData.state !== 'ready'" class="hint">不可用</span>
  </button>
</template>

<style scoped>
.artifact-chip {
  display: inline-flex;
  max-width: 100%;
  gap: var(--space-2);
  align-items: center;
  min-height: 32px;
  padding: 0 var(--space-2);
  color: var(--ctp-text);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: 999px;
  cursor: pointer;
}
.artifact-chip:disabled {
  cursor: default;
  color: var(--ctp-subtext0);
}
.name {
  overflow: hidden;
  font-size: var(--font-small);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hint {
  color: var(--ctp-red);
  font-size: var(--font-small);
}
</style>
