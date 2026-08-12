<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type {
  DesktopBridge,
  RecoveryActionKind,
  RecoveryActionPlan,
  RecoveryBundleRecord,
  RecoveryHistory,
  RecoveryIssue,
} from "../../bridge/types";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";

const props = defineProps<{ bridge: DesktopBridge }>();

const issues = ref<RecoveryIssue[]>([]);
const selectedId = ref<string>();
const plan = ref<RecoveryActionPlan>();
const approved = ref(false);
const loading = ref(false);
const error = ref<string>();
const report = ref<string>();
const scanCount = ref(0);
const bundles = ref<RecoveryBundleRecord[]>([]);

const selected = computed(() => issues.value.find((issue) => issue.issueId === selectedId.value));
const activeIssues = computed(() => issues.value.filter((issue) => !["resolved", "retained"].includes(issue.status)));
const completedIssues = computed(() => issues.value.filter((issue) => ["resolved", "retained"].includes(issue.status)));
const groups = computed(() => [
  { id: "immediate", title: "需立即处理", items: activeIssues.value.filter((item) => item.severity === "immediate") },
  { id: "deferred", title: "可安全延后", items: activeIssues.value.filter((item) => item.severity === "deferred") },
  { id: "informational", title: "仅信息", items: activeIssues.value.filter((item) => item.severity === "informational") },
]);

const actionLabels: Record<RecoveryActionKind, string> = {
  mark_interrupted: "标记中断",
  reregister: "重新登记",
  retain: "稍后处理",
  show_location: "打开位置",
  resume_session: "恢复会话",
  continue_integration: "继续集成",
  abort_integration: "中止集成",
  verify_and_cleanup: "验证并清理",
  restore_bundle: "还原备份",
  delete_bundle: "删除备份",
};

/** Short explanations under action buttons for non-expert users. */
const actionHints: Partial<Record<RecoveryActionKind, string>> = {
  reregister: "把资源重新挂回应用登记表，不修改磁盘内容。",
  verify_and_cleanup: "校验安全条件后删除受管资源；失败则保持原状。",
  continue_integration: "从上次中断的合并步骤继续，不自动推送远程。",
  abort_integration: "停止集成并保留恢复包，不修改目标分支。",
  restore_bundle: "用已保存的备份恢复文件与提交。",
  delete_bundle: "永久删除备份包（不可恢复）。",
  retain: "标记为暂不处理，可稍后再扫。",
  show_location: "在资源管理器中显示相关路径。",
  mark_interrupted: "把任务状态标为中断，便于从列表恢复。",
  resume_session: "尝试重新连接已有会话。",
};

function replaceIssue(issue: RecoveryIssue): void {
  const index = issues.value.findIndex((item) => item.issueId === issue.issueId);
  if (index < 0) issues.value.push(issue);
  else issues.value[index] = issue;
}

function resultData<T>(value: unknown, key: string): T {
  return (value as Record<string, T>)[key];
}

async function loadHistory(): Promise<void> {
  const response = await props.bridge.execute({ type: "recovery.history", payload: {} });
  if (response.success === "false") throw new Error(response.error.message);
  const history = resultData<RecoveryHistory>(response.data, "history");
  scanCount.value = history.scans.length;
  bundles.value = history.bundles;
  const latest = new Map<string, RecoveryIssue>();
  for (const issue of history.issues) {
    const previous = latest.get(issue.issueId);
    if (!previous || issue.revision > previous.revision) latest.set(issue.issueId, issue);
  }
  issues.value = [...latest.values()].sort((left, right) => right.detectedAt.localeCompare(left.detectedAt));
  selectedId.value ??= activeIssues.value[0]?.issueId;
}

async function scan(): Promise<void> {
  loading.value = true;
  error.value = undefined;
  report.value = undefined;
  plan.value = undefined;
  try {
    const response = await props.bridge.execute({ type: "recovery.scan", payload: { triggerKind: "manual" } });
    if (response.success === "false") throw new Error(response.error.message);
    issues.value = resultData<RecoveryIssue[]>(response.data, "issues");
    selectedId.value = issues.value[0]?.issueId;
    scanCount.value += 1;
    report.value = `诊断完成：发现 ${issues.value.length} 项，未执行任何清理。`;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function prepare(action: RecoveryActionKind): Promise<void> {
  const issue = selected.value;
  if (!issue) return;
  loading.value = true;
  error.value = undefined;
  report.value = undefined;
  approved.value = false;
  try {
    const response = await props.bridge.execute({
      type: "recovery.prepareAction",
      payload: { issueId: issue.issueId, revision: issue.revision, action },
    });
    if (response.success === "false") throw new Error(response.error.message);
    plan.value = resultData<RecoveryActionPlan>(response.data, "plan");
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function execute(): Promise<void> {
  const ready = plan.value;
  if (!ready || (ready.destructiveLevel === "destructive" && !approved.value)) return;
  loading.value = true;
  error.value = undefined;
  try {
    const response = await props.bridge.execute({
      type: "recovery.executeAction",
      payload: { planId: ready.id, approvalDigest: ready.approvalDigest },
    });
    if (response.success === "false") throw new Error(response.error.message);
    const issue = resultData<RecoveryIssue>(response.data, "issue");
    replaceIssue(issue);
    report.value = `操作完成：${actionLabels[ready.actionKind]}，状态 ${issue.status}。`;
    plan.value = undefined;
    approved.value = false;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function retainDeferred(): Promise<void> {
  const candidates = activeIssues.value.filter((item) => item.severity !== "immediate" && item.safeActions.includes("retain"));
  let completed = 0;
  let skipped = 0;
  loading.value = true;
  error.value = undefined;
  for (const issue of candidates) {
    const prepared = await props.bridge.execute({ type: "recovery.prepareAction", payload: { issueId: issue.issueId, revision: issue.revision, action: "retain" } });
    if (prepared.success === "false") { skipped += 1; continue; }
    const itemPlan = resultData<RecoveryActionPlan>(prepared.data, "plan");
    const executed = await props.bridge.execute({ type: "recovery.executeAction", payload: { planId: itemPlan.id, approvalDigest: itemPlan.approvalDigest } });
    if (executed.success === "false") { skipped += 1; continue; }
    replaceIssue(resultData<RecoveryIssue>(executed.data, "issue"));
    completed += 1;
  }
  report.value = `批量保留完成：${completed} 项；因状态变化或失败跳过 ${skipped} 项。`;
  loading.value = false;
}

onMounted(async () => {
  loading.value = true;
  try {
    await loadHistory();
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <main class="recovery-center" aria-labelledby="recovery-title">
    <header class="page-header">
      <div>
        <p class="eyebrow">UI-RECOVERY-001</p>
        <h1 id="recovery-title">恢复中心</h1>
        <p>先扫描诊断并保全证据，再对单项资源执行安全操作（不会自动推送远程）。</p>
      </div>
      <div class="header-actions">
        <Badge tone="neutral">历史扫描 {{ scanCount }}</Badge>
        <Button :state="loading ? 'loading' : 'default'" data-testid="recovery-scan" @click="scan">重新扫描</Button>
        <Button :disabled="loading || !activeIssues.some((item) => item.severity !== 'immediate')" @click="retainDeferred">保留全部低风险项</Button>
      </div>
    </header>

    <p v-if="activeIssues.length" class="banner" role="status">发现 {{ activeIssues.length }} 项待处理资源；查看不受阻，相关危险操作保持关闭直到计划通过。</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="report" class="report" role="status">{{ report }}</p>

    <div class="recovery-layout">
      <section class="issue-list" aria-label="诊断列表">
        <section v-for="group in groups" :key="group.id" class="issue-group">
          <h2>{{ group.title }} <span>{{ group.items.length }}</span></h2>
          <button
            v-for="issue in group.items"
            :key="issue.issueId"
            type="button"
            class="issue-row"
            :class="{ selected: selectedId === issue.issueId }"
            @click="selectedId = issue.issueId; plan = undefined"
          >
            <span><strong>{{ issue.kind }}</strong><small>{{ issue.resourceId }}</small></span>
            <Badge :tone="issue.severity === 'immediate' ? 'danger' : 'warning'">{{ issue.status }} · r{{ issue.revision }}</Badge>
          </button>
          <p v-if="!group.items.length" class="empty">当前无项目</p>
        </section>
      </section>

      <section v-if="selected" class="issue-detail" aria-label="问题证据与操作">
        <div class="detail-heading">
          <div><p class="eyebrow">{{ selected.kind }}</p><h2>{{ selected.resourceId }}</h2></div>
          <Badge :tone="selected.severity === 'immediate' ? 'danger' : 'warning'">{{ selected.severity }}</Badge>
        </div>
        <dl>
          <div><dt>影响</dt><dd>{{ selected.impact }}</dd></div>
          <div><dt>推荐动作</dt><dd>{{ selected.recommendedAction }}</dd></div>
          <div v-if="selected.canonicalPath"><dt>精确位置</dt><dd>{{ selected.canonicalPath }}</dd></div>
          <div><dt>检测时间</dt><dd>{{ selected.detectedAt }}</dd></div>
        </dl>
        <details open><summary>检测证据</summary><pre>{{ JSON.stringify(selected.evidence, null, 2) }}</pre></details>
        <div class="actions" aria-label="安全操作集合">
          <div v-for="action in selected.safeActions" :key="action" class="action-block">
            <Button
              :variant="['verify_and_cleanup', 'abort_integration', 'restore_bundle', 'delete_bundle'].includes(action) ? 'danger' : 'secondary'"
              :disabled="loading"
              @click="prepare(action)"
            >
              {{ actionLabels[action] }}
            </Button>
            <p v-if="actionHints[action]" class="action-hint">{{ actionHints[action] }}</p>
          </div>
        </div>

        <section v-if="plan" class="plan" data-testid="recovery-plan">
          <h3>操作预览</h3>
          <p><strong>资源：</strong>{{ plan.resourceIdentity }}</p>
          <p v-if="plan.canonicalPath"><strong>路径：</strong>{{ plan.canonicalPath }}</p>
          <p><strong>破坏性等级：</strong>{{ plan.destructiveLevel }}</p>
          <ol><li v-for="step in plan.steps" :key="step">{{ step }}</li></ol>
          <pre>{{ JSON.stringify(plan.expectedState, null, 2) }}</pre>
          <label v-if="plan.destructiveLevel === 'destructive'" class="approval">
            <input v-model="approved" type="checkbox" />
            我已核对精确资源、影响和恢复包状态，并批准此计划。
          </label>
          <div class="actions">
            <Button @click="plan = undefined">取消</Button>
            <Button
              :variant="plan.destructiveLevel === 'destructive' ? 'danger' : 'primary'"
              :disabled="plan.destructiveLevel === 'destructive' && !approved"
              :state="loading ? 'loading' : 'default'"
              data-testid="recovery-execute"
              @click="execute"
            >
              确认执行
            </Button>
          </div>
        </section>
      </section>
      <section v-else class="issue-detail empty-detail"><h2>没有待处理问题</h2><p>可重新扫描，历史记录不会被覆盖。</p></section>
    </div>

    <details class="history">
      <summary>历史报告：{{ completedIssues.length }} 项已结束，{{ bundles.length }} 个恢复包</summary>
      <div class="history-grid">
        <article v-for="issue in completedIssues" :key="`${issue.issueId}-${issue.revision}`">
          <strong>{{ issue.kind }}</strong><span>{{ issue.status }} · r{{ issue.revision }}</span><small>{{ issue.detectedAt }}</small>
        </article>
        <article v-for="bundle in bundles" :key="bundle.id">
          <strong>恢复包 {{ bundle.recoveryItemId }}</strong>
          <span>{{ bundle.verified ? "已校验" : "未校验" }} · manifest SHA-256 {{ bundle.manifestSha256 }}</span>
          <small>branch.bundle / tracked.patch / staged.patch / untracked.zip / manifest.json</small>
        </article>
      </div>
    </details>
  </main>
</template>

<style scoped>
.recovery-center { display: grid; gap: var(--space-4); padding: var(--space-5); color: var(--ctp-text); }
.page-header, .detail-heading, .header-actions, .actions { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); flex-wrap: wrap; }
.page-header h1, .page-header p, .detail-heading h2, .detail-heading p, .plan h3, .plan p { margin: 0; }
.page-header h1 { font-size: var(--heading-page); line-height: var(--leading-tight); font-weight: var(--font-weight-semibold); }
.detail-heading h2 { font-size: var(--heading-panel); line-height: var(--leading-tight); }
.page-header > div:first-child { display: grid; gap: var(--space-1); }
.page-header > div:first-child > p:last-child, .empty, .empty-detail p { color: var(--ctp-subtext0); }
.eyebrow { color: var(--ctp-blue); font-size: var(--font-small); letter-spacing: .06em; text-transform: uppercase; }
.banner, .report { margin: 0; padding: var(--space-3); border: 1px solid var(--ctp-blue); border-radius: var(--radius-panel); background: var(--overlay-info-solid); }
.action-block { display: grid; gap: 2px; }
.action-hint { margin: 0; max-width: 220px; color: var(--ctp-subtext0); font-size: var(--font-small); line-height: var(--leading-tight); }
.error { margin: 0; color: var(--ctp-red); }
.recovery-layout { display: grid; grid-template-columns: minmax(260px, .85fr) minmax(360px, 1.4fr); gap: var(--space-4); min-height: 480px; }
.issue-list, .issue-detail { padding: var(--space-3); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-panel); background: var(--ctp-mantle); }
.issue-list, .issue-group, .issue-detail, .plan { display: grid; align-content: start; gap: var(--space-3); }
.issue-group h2 { display: flex; justify-content: space-between; margin: 0; font-size: var(--font-body); }
.issue-group h2 span { color: var(--ctp-overlay1); }
.issue-row { display: flex; justify-content: space-between; gap: var(--space-2); width: 100%; padding: var(--space-3); color: var(--ctp-text); text-align: left; background: var(--ctp-base); border: 1px solid transparent; border-radius: var(--radius-control); cursor: pointer; }
.issue-row.selected { border-color: var(--ctp-blue); }
.issue-row > span { display: grid; min-width: 0; }
.issue-row small, dd { color: var(--ctp-subtext0); overflow-wrap: anywhere; }
.issue-detail dl { display: grid; gap: var(--space-2); margin: 0; }
.issue-detail dl div { display: grid; gap: 2px; }
dt { color: var(--ctp-overlay1); font-size: var(--font-small); } dd { margin: 0; }
details { border-top: 1px solid var(--ctp-surface1); padding-top: var(--space-3); }
summary { cursor: pointer; }
pre { max-height: 220px; margin: var(--space-2) 0 0; padding: var(--space-3); overflow: auto; color: var(--ctp-subtext1); background: var(--ctp-crust); border-radius: var(--radius-control); white-space: pre-wrap; overflow-wrap: anywhere; }
.plan { padding: var(--space-3); border: 1px solid var(--ctp-yellow); border-radius: var(--radius-panel); }
.history { padding: var(--space-3); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-panel); background: var(--ctp-mantle); }
.history-grid { display: grid; gap: var(--space-2); margin-top: var(--space-3); }
.history-grid article { display: grid; gap: 2px; padding: var(--space-2); background: var(--ctp-base); border-radius: var(--radius-control); overflow-wrap: anywhere; }
.history-grid span, .history-grid small { color: var(--ctp-subtext0); }
.approval { display: flex; align-items: start; gap: var(--space-2); color: var(--ctp-yellow); }
@media (max-width: 900px) { .recovery-layout { grid-template-columns: 1fr; } }
</style>
