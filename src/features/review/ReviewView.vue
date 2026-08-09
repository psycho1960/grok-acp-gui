<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type {
  CheckpointRecord,
  CheckpointReceipt,
  CheckpointSelection,
  DesktopBridge,
  DiffDocument,
  FileChange,
  IntegrationAttempt,
  IntegrationPlan,
  ReviewSnapshot,
  TaskId,
} from "../../bridge/types";
import Button from "../../shared/ui/Button.vue";
import EmptyState from "../../shared/ui/EmptyState.vue";

const props = defineProps<{ bridge: DesktopBridge; taskId: TaskId }>();

const loading = ref(true);
const committing = ref(false);
const error = ref("");
const query = ref("");
const snapshot = ref<ReviewSnapshot>();
const selected = ref(new Map<string, string>());
const activePath = ref("");
const document = ref<DiffDocument>();
const message = ref("chore(GAG-012): create checkpoint [GAG-012]");
const receipt = ref<CheckpointReceipt>();
const checkpoints = ref<CheckpointRecord[]>([]);
const integrationMessage = ref("feat(GAG-013): squash task checkpoints");
const integrationPlan = ref<IntegrationPlan>();
const integrationAttempt = ref<IntegrationAttempt>();
const integrationApproved = ref(false);
const integrating = ref(false);

const conflictFiles = computed<string[]>(() => parseStringArray(integrationAttempt.value?.conflictSummaryJson));
const validationResults = computed<string[]>(() => parseValidationResults(integrationAttempt.value?.validationResultJson));
const activeAttemptId = computed(() => integrationAttempt.value?.id ?? integrationPlan.value?.attemptId);
const activeApprovalDigest = computed(() => integrationAttempt.value?.approvalDigest ?? integrationPlan.value?.approvalDigest);

const visibleFiles = computed(() => {
  const needle = query.value.trim().toLocaleLowerCase();
  return (snapshot.value?.files ?? []).filter((file) =>
    !needle || file.path.toLocaleLowerCase().includes(needle),
  );
});
const selection = computed<CheckpointSelection[]>(() =>
  [...selected.value].map(([path, fingerprint]) => ({ path, fingerprint })),
);
const selectableCount = computed(() => selection.value.length);

async function load(): Promise<void> {
  loading.value = true;
  error.value = "";
  const [statusResult, historyResult, integrationResult] = await Promise.all([
    props.bridge.execute({ type: "review.status", payload: { taskId: props.taskId } }),
    props.bridge.execute({ type: "review.checkpoints", payload: { taskId: props.taskId } }),
    props.bridge.execute({ type: "integration.active", payload: { taskId: props.taskId } }),
  ]);
  if (statusResult.success === "false") error.value = statusResult.error.message;
  else snapshot.value = (statusResult.data as { snapshot: ReviewSnapshot }).snapshot;
  if (historyResult.success === "true") {
    checkpoints.value = (historyResult.data as { checkpoints: CheckpointRecord[] }).checkpoints;
  }
  if (integrationResult.success === "true") {
    integrationAttempt.value = (integrationResult.data as { attempt?: IntegrationAttempt }).attempt;
  } else if (!error.value) {
    error.value = integrationResult.error.message;
  }
  loading.value = false;
}

function toggle(file: FileChange): void {
  if (file.conflicted || file.submodule) return;
  const next = new Map(selected.value);
  if (next.has(file.path)) next.delete(file.path);
  else next.set(file.path, file.fingerprint);
  selected.value = next;
}

function isStale(file: FileChange): boolean {
  const fingerprint = selected.value.get(file.path);
  return fingerprint !== undefined && fingerprint !== file.fingerprint;
}

async function openDiff(file: FileChange): Promise<void> {
  activePath.value = file.path;
  document.value = undefined;
  error.value = "";
  const result = await props.bridge.execute({
    type: "review.diff",
    payload: { taskId: props.taskId, path: file.path, fingerprint: file.fingerprint },
  });
  if (result.success === "false") error.value = result.error.message;
  else document.value = (result.data as { document: DiffDocument }).document;
}

async function createCheckpoint(): Promise<void> {
  if (!selection.value.length || committing.value) return;
  committing.value = true;
  error.value = "";
  const validation = await props.bridge.execute({
    type: "review.validate",
    payload: { taskId: props.taskId, selection: selection.value },
  });
  if (validation.success === "false") {
    error.value = validation.error.message;
    committing.value = false;
    return;
  }
  const result = await props.bridge.execute({
    type: "review.checkpoint",
    payload: { taskId: props.taskId, message: message.value, selection: selection.value },
  });
  if (result.success === "false") {
    error.value = result.error.message;
  } else {
    receipt.value = (result.data as { receipt: CheckpointReceipt }).receipt;
    selected.value = new Map();
    document.value = undefined;
    activePath.value = "";
    await load();
  }
  committing.value = false;
}

async function prepareIntegration(): Promise<void> {
  integrating.value = true; error.value = ""; integrationApproved.value = false; integrationAttempt.value = undefined;
  const result = await props.bridge.execute({ type: "integration.preflight", payload: { taskId: props.taskId, commitMessage: integrationMessage.value } });
  if (result.success === "false") error.value = result.error.message;
  else integrationPlan.value = (result.data as { plan: IntegrationPlan }).plan;
  integrating.value = false;
}

function resetIntegration(): void {
  integrationPlan.value = undefined;
  integrationAttempt.value = undefined;
  integrationApproved.value = false;
  error.value = "";
}

function parseStringArray(value?: string): string[] {
  if (!value) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

function parseValidationResults(value?: string): string[] {
  if (!value) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    if (!Array.isArray(parsed)) return [];
    return parsed.map((item) => typeof item === "string" ? item : JSON.stringify(item));
  } catch {
    return ["验证结果格式无效"];
  }
}

async function startIntegration(): Promise<void> {
  if (!integrationPlan.value || !integrationApproved.value) return;
  integrating.value = true; error.value = "";
  const result = await props.bridge.execute({ type: "integration.execute", payload: { attemptId: integrationPlan.value.attemptId, approvalDigest: integrationPlan.value.approvalDigest } });
  if (result.success === "false") { error.value = result.error.message; await refreshIntegrationStatus(); }
  else integrationAttempt.value = (result.data as { attempt: IntegrationAttempt }).attempt;
  integrating.value = false;
}

async function publishIntegration(): Promise<void> {
  if (!activeAttemptId.value || !activeApprovalDigest.value || integrationAttempt.value?.state !== "ready_to_publish") return;
  integrating.value = true; error.value = "";
  const result = await props.bridge.execute({ type: "integration.publish", payload: { attemptId: activeAttemptId.value, approvalDigest: activeApprovalDigest.value } });
  if (result.success === "false") { error.value = result.error.message; await refreshIntegrationStatus(); }
  else integrationAttempt.value = (result.data as { attempt: IntegrationAttempt }).attempt;
  integrating.value = false;
}

async function refreshIntegrationStatus(): Promise<void> {
  if (!activeAttemptId.value) return;
  const result = await props.bridge.execute({ type: "integration.status", payload: { attemptId: activeAttemptId.value } });
  if (result.success === "true") integrationAttempt.value = (result.data as { attempt: IntegrationAttempt }).attempt;
}

async function abortIntegration(): Promise<void> {
  if (!activeAttemptId.value) return;
  integrating.value = true; error.value = "";
  const result = await props.bridge.execute({ type: "integration.abort", payload: { attemptId: activeAttemptId.value } });
  if (result.success === "false") error.value = result.error.message;
  else integrationAttempt.value = (result.data as { attempt: IntegrationAttempt }).attempt;
  integrating.value = false;
}

async function cleanupIntegration(): Promise<void> {
  if (!activeAttemptId.value) return;
  integrating.value = true; error.value = "";
  const result = await props.bridge.execute({ type: "integration.cleanup", payload: { attemptId: activeAttemptId.value } });
  if (result.success === "false") error.value = result.error.message;
  else integrationAttempt.value = (result.data as { attempt: IntegrationAttempt }).attempt;
  integrating.value = false;
}

async function openIntegrationWorktree(): Promise<void> {
  if (!activeAttemptId.value) return;
  error.value = "";
  const result = await props.bridge.execute({ type: "integration.openWorktree", payload: { attemptId: activeAttemptId.value } });
  if (result.success === "false") error.value = result.error.message;
}

onMounted(load);
</script>

<template>
  <section class="review" aria-labelledby="review-title">
    <header class="review-header">
      <div><h1 id="review-title">变更审查</h1><p>选择文件并创建可追踪的 Checkpoint。</p></div>
      <Button variant="secondary" :disabled="loading || committing" @click="load">刷新</Button>
    </header>
    <p v-if="error" class="notice error" role="alert">{{ error }}</p>
    <p v-if="receipt" class="notice success" role="status">Checkpoint 已创建：{{ receipt.commitSha.slice(0, 12) }}，剩余 {{ receipt.remainingFiles.length }} 个文件。</p>
    <div v-if="loading" class="loading" role="status">正在读取 Worktree 变更…</div>
    <EmptyState v-else-if="!snapshot?.files.length && !checkpoints.length" title="没有待审查变更" detail="当前任务 Worktree 是干净的。" />
    <div v-else class="review-grid">
      <aside class="files" aria-label="变更文件">
        <input v-model="query" type="search" placeholder="搜索文件" aria-label="搜索变更文件" />
        <ul>
          <li v-for="file in visibleFiles" :key="file.path" :class="{ active: activePath === file.path, stale: isStale(file) }">
            <label>
              <input type="checkbox" :checked="selected.has(file.path)" :aria-invalid="isStale(file)" :disabled="file.conflicted || file.submodule" @change="toggle(file)" />
              <button type="button" @click="openDiff(file)">
                <span class="path">{{ file.path }}</span>
                <span class="meta">{{ file.kind }}<template v-if="file.binary"> · 二进制</template><template v-if="file.conflicted"> · 冲突</template><template v-if="file.submodule"> · 子模块</template><template v-if="isStale(file)"> · 已过期</template></span>
              </button>
            </label>
          </li>
        </ul>
      </aside>
      <main class="diff" aria-label="Diff 预览">
        <EmptyState v-if="!activePath" title="选择文件查看 Diff" detail="二进制文件只显示元数据。" />
        <div v-else-if="!document" class="loading">正在加载 Diff…</div>
        <div v-else-if="document.binary" class="binary"><h2>{{ document.path }}</h2><p>二进制文件 · {{ document.bytes }} 字节，不显示文本内容。</p></div>
        <div v-else-if="document.oversized && !document.text" class="binary"><h2>{{ document.path }}</h2><p>Diff 超过安全预览限制，请缩小变更后重试。</p></div>
        <div v-else><h2>{{ document.path }}</h2><p v-if="document.truncated" class="notice">预览已截断。</p><pre>{{ document.text }}</pre></div>
      </main>
      <aside class="checkpoint" aria-label="Checkpoint 摘要">
        <h2>Checkpoint</h2>
        <p>已选择 {{ selectableCount }} 个文件</p>
        <label>提交说明<textarea v-model="message" rows="4" :disabled="committing" /></label>
        <Button variant="primary" :disabled="!selectableCount || committing" @click="createCheckpoint">{{ committing ? "正在提交…" : `创建 Checkpoint（${selectableCount}）` }}</Button>
        <section class="history"><h3>历史</h3><p v-if="!checkpoints.length">尚无 Checkpoint</p><ol v-else><li v-for="item in checkpoints" :key="item.id"><code>{{ item.commitSha.slice(0, 10) }}</code><span>{{ item.message }}</span></li></ol></section>
        <section class="integration" aria-labelledby="integration-title">
          <h3 id="integration-title">Squash 集成</h3>
          <label>集成提交说明<textarea v-model="integrationMessage" rows="3" :disabled="integrating || !!integrationPlan || !!integrationAttempt" /></label>
          <Button v-if="!integrationPlan && !integrationAttempt" variant="secondary" :disabled="integrating || !checkpoints.length" @click="prepareIntegration">集成预检</Button>
          <template v-else>
            <template v-if="integrationPlan">
              <dl><dt>来源</dt><dd><code>{{ integrationPlan.sourceTipSha.slice(0, 12) }}</code>（{{ integrationPlan.sourceRange.length }} 个 Checkpoint）</dd><dt>未选择变更</dt><dd>{{ integrationPlan.sourceDirty ? "存在；保留在来源 Worktree，不进入本次 Squash，状态变化会使审批失效" : "无" }}</dd><dt>目标</dt><dd>{{ integrationPlan.targetRef }} @ <code>{{ integrationPlan.expectedTargetSha.slice(0, 12) }}</code></dd><dt>预计文件</dt><dd><ul><li v-for="file in integrationPlan.expectedFiles" :key="file"><code>{{ file }}</code></li></ul></dd><dt>验证</dt><dd>{{ integrationPlan.validationCommands.length ? integrationPlan.validationCommands.map((item) => item.join(" ")).join("；") : "无已配置命令" }}</dd></dl>
              <ul class="preflight-checklist" aria-label="集成预检清单"><li>来源 Checkpoint 范围已冻结</li><li>目标引用与 HEAD 已校验</li><li>目标分支未在 Worktree 中检出</li><li>来源未选择变更摘要已绑定审批</li></ul>
              <label v-if="!integrationAttempt"><input v-model="integrationApproved" type="checkbox" /> 我确认以上 source、target、提交说明与操作摘要</label>
              <Button v-if="!integrationAttempt" variant="primary" :disabled="!integrationApproved || integrating" @click="startIntegration">{{ integrating ? "正在隔离试合并…" : "开始隔离 Squash" }}</Button>
            </template>
            <p v-if="integrationAttempt && !integrationPlan" class="notice">已恢复未完成的集成尝试。</p>
            <p v-if="integrationAttempt" class="integration-state" role="status">阶段：{{ integrationAttempt.state }}</p>
            <p v-if="integrationAttempt?.state === 'conflicted'">冲突文件：{{ conflictFiles.join("、") }}<br />临时 Worktree：{{ integrationAttempt.temporaryWorktreePath }}</p>
            <Button v-if="integrationAttempt?.state === 'conflicted'" variant="secondary" :disabled="integrating" @click="openIntegrationWorktree">打开临时 Worktree 人工处理</Button>
            <div v-if="validationResults.length"><h4>验证结果</h4><ul><li v-for="result in validationResults" :key="result">{{ result }}</li></ul></div>
            <p v-if="integrationAttempt?.state === 'completed'">已发布 {{ integrationAttempt.resultCommitSha?.slice(0, 12) }}；来源 Worktree 保留，清理状态 {{ integrationAttempt.cleanupStatus }}。</p>
            <Button v-if="integrationAttempt?.state === 'ready_to_publish'" variant="primary" :disabled="integrating" @click="publishIntegration">原子发布到目标引用</Button>
            <Button v-if="integrationAttempt && !['completed','aborted'].includes(integrationAttempt.state)" variant="secondary" :disabled="integrating" @click="abortIntegration">中止并保留恢复包</Button>
            <Button v-if="integrationAttempt && ['completed','aborted','conflicted','validation_failed','publish_rejected','cleanup_required','staging'].includes(integrationAttempt.state) && integrationAttempt.cleanupStatus !== 'completed'" variant="secondary" :disabled="integrating" @click="cleanupIntegration">清理临时资源</Button>
            <Button v-if="integrationAttempt?.cleanupStatus === 'completed' && ['publish_rejected','preflight_failed','aborted'].includes(integrationAttempt.state)" variant="secondary" :disabled="integrating" @click="resetIntegration">目标或来源已变化，重新预检</Button>
          </template>
        </section>
      </aside>
    </div>
  </section>
</template>

<style scoped>
.review { display:grid; gap:var(--space-4); min-height:100%; }.review-header { display:flex; align-items:flex-start; justify-content:space-between; gap:var(--space-4); }.review-header h1,.review-header p,.checkpoint h2,.checkpoint h3,.diff h2 { margin:0; }.review-header p,.meta,.checkpoint p,.history { color:var(--ctp-subtext0); }.review-grid { display:grid; grid-template-columns:minmax(220px, 28%) minmax(320px, 1fr) minmax(250px, 28%); min-height:560px; border:1px solid var(--ctp-surface0); border-radius:var(--radius-panel); overflow:hidden; }.files,.checkpoint { padding:var(--space-3); background:var(--ctp-mantle); }.files { border-right:1px solid var(--ctp-surface0); }.checkpoint { display:grid; align-content:start; gap:var(--space-3); border-left:1px solid var(--ctp-surface0); }.files input[type="search"],textarea { box-sizing:border-box; width:100%; color:var(--ctp-text); background:var(--ctp-surface0); border:1px solid var(--ctp-surface1); border-radius:var(--radius-control); padding:var(--space-2); }.files ul,.history ol { margin:var(--space-3) 0 0; padding:0; list-style:none; }.files li { border-radius:var(--radius-control); }.files li.active { background:var(--ctp-surface0); }.files label { display:flex; align-items:flex-start; gap:var(--space-2); padding:var(--space-2); }.files button { min-width:0; padding:0; color:var(--ctp-text); text-align:left; background:transparent; border:0; cursor:pointer; }.path,.meta { display:block; overflow-wrap:anywhere; }.meta { margin-top:var(--space-1); font-size:var(--font-small); }.diff { min-width:0; padding:var(--space-4); overflow:auto; }.diff pre { margin:var(--space-3) 0 0; color:var(--ctp-text); white-space:pre; font-family:var(--font-mono); font-size:var(--font-small); }.notice,.loading,.binary { padding:var(--space-3); border-radius:var(--radius-control); background:var(--ctp-surface0); }.error { color:var(--ctp-red); }.success { color:var(--ctp-green); }.checkpoint label { display:grid; gap:var(--space-2); color:var(--ctp-subtext0); }.history { border-top:1px solid var(--ctp-surface0); padding-top:var(--space-3); }.history li { display:grid; gap:var(--space-1); margin-top:var(--space-2); }.history code { color:var(--ctp-mauve); }@media (max-width:1200px) { .review-grid { grid-template-columns:minmax(220px, 34%) 1fr; }.checkpoint { grid-column:1 / -1; border-top:1px solid var(--ctp-surface0); border-left:0; } }@media (max-width:760px) { .review-grid { display:block; }.files,.checkpoint { border:0; border-bottom:1px solid var(--ctp-surface0); }.diff { min-height:320px; } }
</style>
