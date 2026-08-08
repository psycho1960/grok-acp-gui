<script setup lang="ts">
import { ref, watch } from "vue";
import Button from "../../shared/ui/Button.vue";
import type { ArtifactDescriptor, DesktopBridge, TaskId } from "../../bridge/types";
import { createConversationFacade } from "./conversation-facade";

const props = defineProps<{
  bridge: DesktopBridge;
  taskId: TaskId | null;
  refreshKey: number;
}>();

const facade = createConversationFacade(props.bridge);
const artifacts = ref<ArtifactDescriptor[]>([]);
const urls = ref<Record<string, string>>({});
const loading = ref(false);
const actionId = ref<string | null>(null);
const error = ref<string | null>(null);

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.ceil(bytes / 1024)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

async function refresh(): Promise<void> {
  if (!props.taskId) {
    artifacts.value = [];
    urls.value = {};
    return;
  }
  loading.value = true;
  error.value = null;
  try {
    const result = await facade.listArtifacts(props.taskId);
    if (result.success === "false") throw new Error(result.error.message);
    artifacts.value = result.data?.artifacts ?? [];
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "无法加载 Artifact";
  } finally {
    loading.value = false;
  }
}

async function preview(artifact: ArtifactDescriptor): Promise<void> {
  if (!props.taskId || artifact.previewCapability === "none") return;
  actionId.value = artifact.artifactId;
  error.value = null;
  try {
    const result = await facade.previewArtifact(props.taskId, artifact.artifactId);
    if (result.success === "false") throw new Error(result.error.message);
    if (result.data?.url) urls.value = { ...urls.value, [artifact.artifactId]: result.data.url };
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "无法加载安全预览";
  } finally {
    actionId.value = null;
  }
}

async function reveal(artifact: ArtifactDescriptor): Promise<void> {
  if (!props.taskId) return;
  actionId.value = artifact.artifactId;
  error.value = null;
  try {
    const result = await facade.revealArtifact(props.taskId, artifact.artifactId);
    if (result.success === "false") throw new Error(result.error.message);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "无法在资源管理器中显示";
  } finally {
    actionId.value = null;
  }
}

function openArtifact(artifactId: string): void {
  const artifact = artifacts.value.find((candidate) => candidate.artifactId === artifactId);
  if (artifact) void preview(artifact);
}

watch(() => [props.taskId, props.refreshKey], () => void refresh(), { immediate: true });

defineExpose({ openArtifact, refresh });
</script>

<template>
  <aside class="artifact-panel" aria-label="制品面板" data-testid="artifact-panel">
    <header>
      <div>
        <h2>制品</h2>
        <p>受管副本，原始路径不会暴露给页面。</p>
      </div>
      <Button variant="ghost" :disabled="loading" @click="refresh">刷新</Button>
    </header>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-else-if="loading" class="empty">正在加载…</p>
    <p v-else-if="!artifacts.length" class="empty">此任务尚无可用制品。</p>
    <ul v-else class="artifact-list">
      <li v-for="artifact in artifacts" :key="artifact.artifactId" class="artifact-card">
        <div class="metadata">
          <strong>{{ artifact.displayName }}</strong>
          <span>{{ artifact.mimeType }} · {{ formatBytes(artifact.bytes) }}</span>
          <span :class="`state state-${artifact.state}`">{{ artifact.state }}</span>
        </div>
        <img
          v-if="urls[artifact.artifactId]"
          :src="urls[artifact.artifactId]"
          :alt="artifact.displayName"
          class="preview"
        >
        <div class="actions">
          <Button
            :state="actionId === artifact.artifactId ? 'loading' : 'default'"
            :disabled="artifact.previewCapability === 'none'"
            @click="preview(artifact)"
          >
            {{ artifact.previewCapability === 'onDemand' ? '加载预览' : '预览' }}
          </Button>
          <Button :disabled="artifact.state !== 'ready'" @click="reveal(artifact)">
            显示位置
          </Button>
        </div>
      </li>
    </ul>
  </aside>
</template>

<style scoped>
.artifact-panel { min-width: 0; overflow: auto; padding: var(--space-3); border-left: 1px solid var(--ctp-surface0); background: var(--ctp-mantle); }
header { display: flex; align-items: start; justify-content: space-between; gap: var(--space-2); }
h2, p { margin: 0; } h2 { font-size: var(--font-body); } header p, .empty { margin-top: var(--space-1); color: var(--ctp-subtext0); font-size: var(--font-small); }
.artifact-list { display: grid; gap: var(--space-2); padding: 0; margin: var(--space-3) 0 0; list-style: none; }
.artifact-card { display: grid; gap: var(--space-2); padding: var(--space-2); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-card); background: var(--ctp-base); }
.metadata { display: grid; gap: 2px; overflow-wrap: anywhere; } .metadata span { color: var(--ctp-subtext0); font-size: var(--font-small); }
.state { text-transform: capitalize; } .state-missing, .state-quarantined, .error { color: var(--ctp-red) !important; }
.preview { display: block; width: 100%; max-height: 260px; object-fit: contain; border-radius: var(--radius-control); background: var(--ctp-crust); }
.actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.error { margin-top: var(--space-2); font-size: var(--font-small); }
</style>
