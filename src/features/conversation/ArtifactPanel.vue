<script setup lang="ts">
import { ref, watch } from "vue";
import Button from "../../shared/ui/Button.vue";
import type { ArtifactDescriptor, ArtifactSaveResult, DesktopBridge, TaskId } from "../../bridge/types";
import { pickArtifactSavePath } from "../../bridge/artifact-save-picker";
import { createConversationFacade } from "./conversation-facade";

const props = defineProps<{
  bridge: DesktopBridge;
  taskId: TaskId | null;
  refreshKey: number;
  focusArtifactId?: string | null;
}>();

const facade = createConversationFacade(props.bridge);
const artifacts = ref<ArtifactDescriptor[]>([]);
const urls = ref<Record<string, string>>({});
const loading = ref(false);
const actionId = ref<string | null>(null);
const error = ref<string | null>(null);
const notice = ref<string | null>(null);
const conflict = ref<{ artifact: ArtifactDescriptor; targetPath: string; result: ArtifactSaveResult } | null>(null);
const saved = ref<{ artifactId: string; targetPath: string; targetName: string } | null>(null);

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

async function commitSave(
  artifact: ArtifactDescriptor,
  targetPath: string,
  overwrite: boolean,
): Promise<void> {
  if (!props.taskId) return;
  actionId.value = artifact.artifactId;
  error.value = null;
  notice.value = null;
  try {
    const response = await facade.saveArtifact(
      props.taskId,
      artifact.artifactId,
      targetPath,
      overwrite,
    );
    if (response.success === "false") throw new Error(response.error.message);
    const result = response.data;
    if (result.status === "conflict") {
      conflict.value = { artifact, targetPath, result };
      return;
    }
    conflict.value = null;
    if (result.status === "saved") {
      const targetName = result.targetName ?? artifact.displayName;
      saved.value = { artifactId: artifact.artifactId, targetPath, targetName };
      notice.value = result.extensionWarning
        ? `已保存 ${targetName}。${result.extensionWarning}`
        : `已保存 ${targetName}`;
      return;
    }
    if (result.status === "cancelled") {
      notice.value = "已取消另存为，未修改任何文件";
      return;
    }
    error.value = result.message ?? (result.status === "rejected" ? "该制品或目标位置不允许保存" : "保存失败，已有目标未被修改");
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存失败，已有目标未被修改";
  } finally {
    actionId.value = null;
  }
}

async function saveAs(artifact: ArtifactDescriptor): Promise<void> {
  if (artifact.state !== "ready") return;
  error.value = null;
  notice.value = null;
  saved.value = null;
  const selection = await pickArtifactSavePath(artifact);
  if (selection.status === "failed") {
    error.value = selection.message ?? "无法打开系统保存对话框，请重试";
    return;
  }
  if (selection.status === "cancelled") {
    conflict.value = null;
    notice.value = selection.message ?? "已取消另存为，未修改任何文件";
    return;
  }
  await commitSave(artifact, selection.path, false);
}

function cancelConflict(): void {
  conflict.value = null;
  saved.value = null;
  notice.value = "已取消覆盖，原文件未被修改";
}

async function renameConflict(): Promise<void> {
  const pending = conflict.value;
  conflict.value = null;
  if (pending) await saveAs(pending.artifact);
}

async function overwriteConflict(): Promise<void> {
  const pending = conflict.value;
  if (!pending) return;
  await commitSave(pending.artifact, pending.targetPath, true);
}

async function revealSaved(): Promise<void> {
  if (!props.taskId || !saved.value) return;
  error.value = null;
  const target = saved.value;
  try {
    const result = await facade.revealSavedArtifact(
      props.taskId,
      target.artifactId,
      target.targetPath,
    );
    if (result.success === "false") throw new Error(result.error.message);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "无法在资源管理器中显示保存结果";
  }
}

function openArtifact(artifactId: string): void {
  const artifact = artifacts.value.find((candidate) => candidate.artifactId === artifactId);
  if (artifact) {
    void preview(artifact);
    return;
  }
  void preview({
    artifactId,
    displayName: artifactId,
    mimeType: "image/*",
    bytes: 0,
    state: "ready",
    previewCapability: "inline",
  });
}

watch(() => [props.taskId, props.refreshKey], () => void refresh(), { immediate: true });

watch(
  () => props.focusArtifactId,
  (artifactId) => {
    if (artifactId) openArtifact(artifactId);
  },
);

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
    <div v-if="notice" class="notice" role="status" data-testid="artifact-save-notice">
      <span>{{ notice }}</span>
      <Button v-if="saved" variant="ghost" data-testid="reveal-saved-artifact" @click="revealSaved">
        打开所在位置
      </Button>
    </div>
    <p v-else-if="loading" class="empty">正在加载…</p>
    <p v-else-if="!artifacts.length" class="empty">此任务尚无可用制品。</p>
    <ul v-else class="artifact-list" data-testid="artifact-gallery">
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
          :data-testid="artifact.artifactId === focusArtifactId ? 'focused-artifact-preview' : undefined"
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
          <Button
            :state="actionId === artifact.artifactId ? 'loading' : 'default'"
            :disabled="artifact.state !== 'ready'"
            data-testid="save-artifact"
            @click="saveAs(artifact)"
          >
            另存为
          </Button>
        </div>
        <div
          v-if="conflict?.artifact.artifactId === artifact.artifactId"
          class="conflict"
          role="alertdialog"
          aria-label="目标文件冲突"
          data-testid="artifact-save-conflict"
        >
          <strong>目标文件已存在</strong>
          <p>{{ conflict.result.message }}</p>
          <p v-if="conflict.result.extensionWarning" class="warning">{{ conflict.result.extensionWarning }}</p>
          <div class="actions">
            <Button data-testid="cancel-overwrite" @click="cancelConflict">取消</Button>
            <Button data-testid="rename-artifact" @click="renameConflict">另存为新名称</Button>
            <Button variant="danger" data-testid="confirm-overwrite" @click="overwriteConflict">明确覆盖</Button>
          </div>
        </div>
      </li>
    </ul>
  </aside>
</template>

<style scoped>
.artifact-panel { min-width: 0; overflow: auto; padding: var(--space-3); background: transparent; }
header { display: flex; align-items: start; justify-content: space-between; gap: var(--space-2); }
h2, p { margin: 0; } h2 { font-size: var(--font-body); } header p, .empty { margin-top: var(--space-1); color: var(--ctp-subtext0); font-size: var(--font-small); }
.artifact-list { display: grid; gap: var(--space-2); padding: 0; margin: var(--space-3) 0 0; list-style: none; }
.artifact-card { display: grid; gap: var(--space-2); padding: var(--space-2); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-card); background: var(--ctp-base); }
.metadata { display: grid; gap: 2px; overflow-wrap: anywhere; } .metadata span { color: var(--ctp-subtext0); font-size: var(--font-small); }
.state { text-transform: capitalize; } .state-missing, .state-quarantined, .error { color: var(--ctp-red) !important; }
.preview { display: block; width: 100%; max-height: 260px; object-fit: contain; border-radius: var(--radius-control); background: var(--ctp-crust); }
.actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.error { margin-top: var(--space-2); font-size: var(--font-small); }
.notice { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); margin-top: var(--space-2); padding: var(--space-2); border: 1px solid var(--ctp-green); border-radius: var(--radius-control); color: var(--ctp-green); font-size: var(--font-small); }
.conflict { display: grid; gap: var(--space-2); padding: var(--space-2); border: 1px solid var(--ctp-yellow); border-radius: var(--radius-control); background: var(--ctp-mantle); }
.conflict p { color: var(--ctp-subtext0); font-size: var(--font-small); }
.warning { color: var(--ctp-yellow) !important; }
</style>
