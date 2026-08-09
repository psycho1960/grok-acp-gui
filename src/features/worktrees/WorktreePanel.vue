<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type {
  AdoptionPreparation,
  DesktopBridge,
  RemovalPreparation,
  TaskId,
  WorktreeRecord,
} from "../../bridge/types";
import Badge from "../../shared/ui/Badge.vue";
import Button from "../../shared/ui/Button.vue";
import Dialog from "../../shared/ui/Dialog.vue";
import Input from "../../shared/ui/Input.vue";

const props = defineProps<{
  bridge: DesktopBridge;
  taskId: TaskId;
}>();

const worktree = ref<WorktreeRecord | null>(null);
const externalWorktrees = ref<WorktreeRecord[]>([]);
const loading = ref(false);
const preparing = ref(false);
const removing = ref(false);
const error = ref<string | null>(null);
const preparation = ref<RemovalPreparation | null>(null);
const confirmedPath = ref("");
const riskAcknowledged = ref(false);
const adoptionPreparation = ref<AdoptionPreparation | null>(null);
const adoptionConfirmedPath = ref("");

const canConfirmRemoval = computed(
  () =>
    preparation.value != null &&
    confirmedPath.value === preparation.value.absolutePath &&
    (!preparation.value.forceRequired || riskAcknowledged.value),
);

const formattedSize = computed(() => {
  const bytes = worktree.value?.diskUsageBytes ?? 0;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
});

async function inspect(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const result = await props.bridge.execute({
      type: "worktree.inspect",
      payload: { taskId: props.taskId },
    });
    if (result.success === "false") {
      error.value = result.error.message;
      return;
    }
    worktree.value = (result.data as { worktree: WorktreeRecord }).worktree;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function reconcile(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const result = await props.bridge.execute({
      type: "worktree.reconcile",
      payload: {},
    });
    if (result.success === "false") {
      error.value = result.error.message;
      return;
    }
    const records = (result.data as { worktrees: WorktreeRecord[] }).worktrees;
    worktree.value = records.find((item) => item.taskId === props.taskId) ?? worktree.value;
    externalWorktrees.value = records.filter((item) => item.ownership === "external");
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function prepareExternalAdoption(path: string): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const result = await props.bridge.execute({
      type: "worktree.prepareAdoption",
      payload: { taskId: props.taskId, path },
    });
    if (result.success === "false") {
      error.value = result.error.message;
      return;
    }
    adoptionPreparation.value = (result.data as { preparation: AdoptionPreparation }).preparation;
    adoptionConfirmedPath.value = "";
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function confirmExternalAdoption(): Promise<void> {
  const ready = adoptionPreparation.value;
  if (!ready || adoptionConfirmedPath.value !== ready.absolutePath) return;
  loading.value = true;
  error.value = null;
  try {
    const result = await props.bridge.execute({
      type: "worktree.adopt",
      payload: {
        taskId: props.taskId,
        path: ready.absolutePath,
        confirmationToken: ready.confirmationToken,
        confirmedPath: adoptionConfirmedPath.value,
      },
    });
    if (result.success === "false") {
      error.value = result.error.message;
      return;
    }
    worktree.value = (result.data as { worktree: WorktreeRecord }).worktree;
    externalWorktrees.value = externalWorktrees.value.filter(
      (item) => item.path !== ready.absolutePath,
    );
    adoptionPreparation.value = null;
    adoptionConfirmedPath.value = "";
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

async function prepareRemoval(): Promise<void> {
  preparing.value = true;
  error.value = null;
  try {
    const result = await props.bridge.execute({
      type: "worktree.prepareRemoval",
      payload: { taskId: props.taskId },
    });
    if (result.success === "false") {
      error.value = result.error.message;
      return;
    }
    preparation.value = (result.data as { preparation: RemovalPreparation }).preparation;
    confirmedPath.value = "";
    riskAcknowledged.value = false;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    preparing.value = false;
  }
}

async function removeWorktree(): Promise<void> {
  const ready = preparation.value;
  if (!ready || !canConfirmRemoval.value) return;
  removing.value = true;
  error.value = null;
  try {
    const result = await props.bridge.execute({
      type: "worktree.remove",
      payload: {
        taskId: props.taskId,
        confirmationToken: ready.confirmationToken,
        confirmedPath: confirmedPath.value,
      },
    });
    if (result.success === "false") {
      error.value = result.error.message;
      return;
    }
    worktree.value = (result.data as { worktree: WorktreeRecord }).worktree;
    preparation.value = null;
    confirmedPath.value = "";
    riskAcknowledged.value = false;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    removing.value = false;
  }
}

async function cancelRemoval(): Promise<void> {
  preparation.value = null;
  confirmedPath.value = "";
  riskAcknowledged.value = false;
  await reconcile();
}

watch(
  () => props.taskId,
  () => {
    preparation.value = null;
    confirmedPath.value = "";
    riskAcknowledged.value = false;
    void inspect();
  },
  { immediate: true },
);
</script>

<template>
  <section class="worktree-panel" aria-labelledby="worktree-panel-title">
    <header>
      <div>
        <h2 id="worktree-panel-title">Worktree</h2>
        <p>受管工作区状态与安全操作</p>
      </div>
      <Badge v-if="worktree" :tone="worktree.state === 'dirty' ? 'warning' : worktree.state === 'missing' || worktree.state === 'quarantined' ? 'danger' : 'neutral'">
        {{ worktree.state }}
      </Badge>
    </header>
    <p v-if="worktree?.ownership === 'adopted'">外部 Worktree 已接管；受管根外仍保持只读清理。</p>

    <p v-if="loading" role="status">正在校验 Worktree…</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <dl v-if="worktree" class="facts">
      <div><dt>任务分支</dt><dd>{{ worktree.branch }}</dd></div>
      <div><dt>基础分支</dt><dd>{{ worktree.baseBranch }}</dd></div>
      <div><dt>绝对路径</dt><dd>{{ worktree.path }}</dd></div>
      <div><dt>脏状态</dt><dd>{{ worktree.state === "dirty" ? "有未提交内容" : "未检测到修改" }}</dd></div>
      <div><dt>磁盘占用</dt><dd>{{ formattedSize }}</dd></div>
      <div><dt>最近校验</dt><dd>{{ worktree.lastVerifiedAt || "尚未记录" }}</dd></div>
      <div><dt>Git 锁</dt><dd>{{ worktree.locked ? "已锁定" : "可用" }}</dd></div>
      <div><dt>关联任务</dt><dd>{{ worktree.taskId }}</dd></div>
    </dl>
    <div class="actions">
      <Button :state="loading ? 'loading' : 'default'" @click="inspect">重新检查</Button>
      <Button :state="loading ? 'loading' : 'default'" @click="reconcile">对账</Button>
      <Button
        v-if="worktree?.ownership === 'managed' && !['removed', 'missing', 'quarantined'].includes(worktree.state)"
        variant="danger"
        :state="preparing ? 'loading' : 'default'"
        data-testid="worktree-prepare-removal"
        @click="prepareRemoval"
      >
        清理 Worktree
      </Button>
    </div>
    <section v-if="externalWorktrees.length" class="external-list" aria-label="外部 Worktree">
      <h3>外部 Worktree（只读）</h3>
      <article v-for="item in externalWorktrees" :key="item.id">
        <div><strong>{{ item.branch }}</strong><span>{{ item.path }}</span></div>
        <Button :disabled="worktree != null" @click="prepareExternalAdoption(item.path)">
          接管到当前任务
        </Button>
      </article>
    </section>
  </section>

  <Dialog
    :model-value="preparation != null"
    title="确认清理 Worktree"
    description="后端已重新校验登记、Git 元数据和受管根。请输入完整绝对路径继续。"
    @update:model-value="!$event && cancelRemoval()"
  >
    <div v-if="preparation" class="removal-summary">
      <p><strong>准确目标：</strong>{{ preparation.absolutePath }}</p>
      <p>脏状态：{{ preparation.dirty ? "有修改" : "干净" }}</p>
      <p>未跟踪文件：{{ preparation.untrackedFiles }}</p>
      <template v-if="preparation.recovery">
        <p class="recovery-ok">恢复包已创建并验证。</p>
        <ul>
          <li>branch.bundle</li>
          <li>tracked.patch</li>
          <li>untracked.zip</li>
          <li>manifest.json</li>
        </ul>
      </template>
      <p v-else>当前内容已合并且干净，不需要恢复包。</p>
      <label v-if="preparation.forceRequired" class="risk-ack">
        <input v-model="riskAcknowledged" type="checkbox" />
        我确认这是未合并或含修改的强制清理，且将依赖上方恢复包恢复。
      </label>
      <Input
        v-model="confirmedPath"
        label="输入上方完整绝对路径"
        :error="confirmedPath && !canConfirmRemoval ? '路径必须逐字匹配' : undefined"
        data-testid="worktree-confirm-path"
      />
    </div>
    <template #actions>
      <Button @click="cancelRemoval">保留</Button>
      <Button
        variant="danger"
        :disabled="!canConfirmRemoval"
        :state="removing ? 'loading' : 'default'"
        data-testid="worktree-confirm-removal"
        @click="removeWorktree"
      >
        删除已验证目标
      </Button>
    </template>
  </Dialog>

  <Dialog
    :model-value="adoptionPreparation != null"
    title="确认接管外部 Worktree"
    description="接管会把该外部路径绑定到当前任务，但不会授予受管清理权限。请输入完整绝对路径确认。"
    @update:model-value="!$event && (adoptionPreparation = null)"
  >
    <div v-if="adoptionPreparation" class="removal-summary">
      <p><strong>外部路径：</strong>{{ adoptionPreparation.absolutePath }}</p>
      <Input
        v-model="adoptionConfirmedPath"
        label="输入上方完整绝对路径"
        data-testid="worktree-confirm-adoption-path"
      />
    </div>
    <template #actions>
      <Button @click="adoptionPreparation = null">取消</Button>
      <Button
        :disabled="adoptionConfirmedPath !== adoptionPreparation?.absolutePath"
        data-testid="worktree-confirm-adoption"
        @click="confirmExternalAdoption"
      >
        确认接管
      </Button>
    </template>
  </Dialog>
</template>

<style scoped>
.worktree-panel { display: grid; gap: var(--space-3); }
.worktree-panel header { display: flex; align-items: start; justify-content: space-between; gap: var(--space-2); }
.worktree-panel h2, .worktree-panel p, .removal-summary p { margin: 0; }
.worktree-panel header p { color: var(--ctp-subtext0); font-size: var(--font-small); }
.facts { display: grid; gap: var(--space-2); margin: 0; }
.facts div { display: grid; gap: 2px; }
.facts dt { color: var(--ctp-overlay0); font-size: var(--font-small); }
.facts dd { margin: 0; color: var(--ctp-text); overflow-wrap: anywhere; }
.actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.external-list { display: grid; gap: var(--space-2); }
.external-list h3 { margin: 0; font-size: var(--font-body); }
.external-list article { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); }
.external-list article div { display: grid; min-width: 0; }
.external-list article span { color: var(--ctp-subtext0); overflow-wrap: anywhere; }
.error { color: var(--ctp-red); }
.removal-summary { display: grid; gap: var(--space-3); overflow-wrap: anywhere; }
.recovery-ok { color: var(--ctp-green); }
.risk-ack { display: flex; align-items: start; gap: var(--space-2); color: var(--ctp-yellow); }
.removal-summary ul { margin: 0; color: var(--ctp-subtext0); }
</style>
