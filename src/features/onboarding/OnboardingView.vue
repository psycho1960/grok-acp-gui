<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type {
  DesktopBridge,
  DesktopResult,
  RuntimeLoginResult,
  RuntimeReadinessSnapshot,
  StartupCheck,
} from "../../bridge/types";
import Button from "../../shared/ui/Button.vue";
import Dialog from "../../shared/ui/Dialog.vue";
import StatusIcon from "../../shared/ui/StatusIcon.vue";

const props = defineProps<{ bridge: DesktopBridge }>();
const emit = defineEmits<{ ready: [snapshot: RuntimeReadinessSnapshot] }>();

const GROK_INSTALL_URL = "https://docs.x.ai/build/getting-started";
const GIT_INSTALL_URL = "https://git-scm.com/download/win";

const orderedChecks: StartupCheck[] = [
  { id: "git", label: "Git", status: "checking", detail: "正在检查 Git…" },
  { id: "grok", label: "Grok", status: "checking", detail: "正在查找 Grok CLI…" },
  { id: "version", label: "版本", status: "checking", detail: "正在检查 Grok 版本…" },
  { id: "authentication", label: "认证", status: "checking", detail: "正在验证 Grok 登录状态…" },
  { id: "database", label: "数据库", status: "checking", detail: "正在检查应用数据库…" },
  { id: "directory", label: "工作目录", status: "checking", detail: "正在检查目录权限…" },
  { id: "acp", label: "ACP 握手", status: "checking", detail: "正在验证 ACP 通道…" },
];

const snapshot = ref<RuntimeReadinessSnapshot | null>(null);
const checks = ref<StartupCheck[]>(orderedChecks);
const login = ref<RuntimeLoginResult>({ status: "idle", retryable: true });
const loading = ref(false);
const retryingCheckId = ref<StartupCheck["id"] | null>(null);
const bridgeError = ref<string | null>(null);
const remediationOpen = ref(false);
const remediationCheck = ref<StartupCheck | null>(null);
let pollTimer: ReturnType<typeof setTimeout> | undefined;

const isLoginRunning = computed(() => login.value.status === "running");
const overallLabel = computed(() => {
  if (loading.value) return "正在检查";
  if (snapshot.value?.ready) return "全部检查通过";
  if (checks.value.some((check) => check.status === "error")) return "需要处理";
  return "等待检查";
});

function dataOf<T>(result: DesktopResult): T {
  if (result.success === "false") throw new Error(result.error.message);
  return result.data as T;
}

async function refresh(checkId: StartupCheck["id"] | null = null): Promise<void> {
  loading.value = true;
  retryingCheckId.value = checkId;
  bridgeError.value = null;
  if (!snapshot.value) checks.value = orderedChecks;
  try {
    const next = dataOf<RuntimeReadinessSnapshot>(
      await props.bridge.execute({ type: "runtime.refresh", payload: {} }),
    );
    snapshot.value = next;
    checks.value = next.checks;
    login.value = next.login;
    if (next.ready) emit("ready", next);
  } catch (error) {
    bridgeError.value = error instanceof Error ? error.message : String(error);
  } finally {
    loading.value = false;
    retryingCheckId.value = null;
  }
}

async function startLogin(): Promise<void> {
  bridgeError.value = null;
  try {
    login.value = dataOf<RuntimeLoginResult>(
      await props.bridge.execute({ type: "runtime.login", payload: { method: "oauth" } }),
    );
    if (login.value.status === "running") scheduleLoginPoll();
  } catch (error) {
    bridgeError.value = error instanceof Error ? error.message : String(error);
  }
}

async function cancelLogin(): Promise<void> {
  clearLoginPoll();
  try {
    login.value = dataOf<RuntimeLoginResult>(
      await props.bridge.execute({ type: "runtime.login", payload: { method: "cancel" } }),
    );
    scheduleLoginPoll();
  } catch (error) {
    bridgeError.value = error instanceof Error ? error.message : String(error);
  }
}

function scheduleLoginPoll(): void {
  clearLoginPoll();
  pollTimer = setTimeout(() => void pollLogin(), 1_000);
}

async function pollLogin(): Promise<void> {
  try {
    login.value = dataOf<RuntimeLoginResult>(
      await props.bridge.execute({ type: "runtime.login", payload: { method: "status" } }),
    );
    if (login.value.status === "running") {
      scheduleLoginPoll();
      return;
    }
    if (login.value.status === "succeeded") await refresh();
  } catch (error) {
    bridgeError.value = error instanceof Error ? error.message : String(error);
  }
}

function clearLoginPoll(): void {
  if (pollTimer !== undefined) clearTimeout(pollTimer);
  pollTimer = undefined;
}

function isUnresolved(check: StartupCheck): boolean {
  return check.status === "warning" || check.status === "error";
}

function canStartLogin(check: StartupCheck): boolean {
  if (isLoginRunning.value || !isUnresolved(check)) return false;
  return (
    check.id === "authentication" ||
    (check.id === "acp" && snapshot.value?.authenticated !== true)
  );
}

function guidanceFor(check: StartupCheck): { href: string; label: string; testId: string } | null {
  if (check.id === "git") {
    return { href: GIT_INSTALL_URL, label: "安装 Git", testId: "git-install-link" };
  }
  if (check.id === "grok" && snapshot.value?.installed === false) {
    return { href: GROK_INSTALL_URL, label: "安装 Grok", testId: "grok-install-link" };
  }
  if (check.id === "version" && check.status === "error") {
    return { href: GROK_INSTALL_URL, label: "查看更新说明", testId: "grok-update-link" };
  }
  if (
    check.id === "acp" ||
    (check.id === "authentication" && !canStartLogin(check))
  ) {
    return { href: GROK_INSTALL_URL, label: "查看 Grok 说明", testId: "grok-help-link" };
  }
  return null;
}

function repairSummary(check: StartupCheck): string {
  switch (check.id) {
    case "git":
      return "安装 Git for Windows 后，返回这里立即复检。";
    case "grok":
      return "安装 Grok CLI 后，返回这里立即复检。";
    case "version":
      return "升级 Grok CLI 后，返回这里立即复检版本和 ACP 能力。";
    case "authentication":
      return "点击“重新登录并修复认证”启动 Grok 官方登录；完成登录后，GAG 会自动复检。";
    case "database":
      return "当前数据异常不会自动删除数据。请先重新检测，仍失败时进入安全恢复流程。";
    case "directory":
      return "修复目录访问权限后，可在这里立即复检。";
    case "acp":
      return "点击“重启 ACP 并复检”重新建立会话；若凭据可能失效，也可直接重新登录 Grok。";
  }
}

function openRemediation(check: StartupCheck): void {
  remediationCheck.value = check;
  remediationOpen.value = true;
}

function closeRemediation(): void {
  remediationOpen.value = false;
}

async function retryFromRemediation(): Promise<void> {
  const checkId = remediationCheck.value?.id ?? null;
  closeRemediation();
  await refresh(checkId);
}

async function loginFromRemediation(): Promise<void> {
  closeRemediation();
  await startLogin();
}

function iconStatus(status: StartupCheck["status"]): "running" | "waiting" | "success" | "error" | "interrupted" {
  if (status === "checking") return "running";
  if (status === "warning") return "interrupted";
  return status;
}

onMounted(() => void refresh());
onBeforeUnmount(clearLoginPoll);
</script>

<template>
  <section class="onboarding" aria-labelledby="onboarding-title">
    <header class="section-heading">
      <div>
        <p class="eyebrow">UI-ONBOARD-001</p>
        <h1 id="onboarding-title">启动检查</h1>
        <p class="intro">确认 Git、Grok、认证和本地数据均可用后再进入任务中心。</p>
      </div>
      <span class="overall" role="status" aria-live="polite">{{ overallLabel }}</span>
    </header>

    <ol class="check-list" aria-label="启动检查结果">
      <li
        v-for="check in checks"
        :key="check.id"
        class="check-row"
        :class="{ 'is-unresolved': isUnresolved(check) }"
        :data-check-id="check.id"
      >
        <StatusIcon :status="iconStatus(check.status)" :label="`${check.label}：${check.status}`" />
        <div class="check-copy">
          <strong>{{ check.label }}</strong>
          <p>{{ check.detail }}</p>
        </div>
        <div v-if="isUnresolved(check)" class="check-actions" :aria-label="`${check.label}处理操作`">
          <Button
            :data-testid="`runtime-check-resolve-${check.id}`"
            :state="loading && retryingCheckId === check.id ? 'loading' : 'default'"
            :disabled="loading"
            @click="openRemediation(check)"
          >
            立即修复
          </Button>
        </div>
      </li>
    </ol>

    <div v-if="isLoginRunning" class="login-state" role="status" aria-live="polite">
      <div>
        <strong>Grok 登录进程正在运行</strong>
        <p>{{ login.message ?? "正在等待 Grok 官方登录流程完成。" }}</p>
      </div>
      <Button data-testid="runtime-login-cancel" variant="ghost" @click="cancelLogin">取消登录</Button>
    </div>
    <p v-else-if="login.status !== 'idle'" class="login-result" role="status">
      {{ login.message }}<span v-if="login.exitCode !== undefined">（退出码 {{ login.exitCode }}）</span>
    </p>

    <p v-if="bridgeError" class="bridge-error" role="alert">{{ bridgeError }}</p>

    <footer class="actions">
      <Button data-testid="runtime-refresh" :state="loading ? 'loading' : 'default'" @click="refresh()">全部重新检测</Button>
    </footer>

    <Dialog
      v-if="remediationCheck"
      v-model="remediationOpen"
      :title="`处理：${remediationCheck.label}`"
      :description="remediationCheck.detail"
    >
      <div class="remediation-page" :data-remediation-id="remediationCheck.id">
        <p class="repair-summary">{{ repairSummary(remediationCheck) }}</p>
        <p v-if="remediationCheck.code" class="error-code">
          错误码：<code>{{ remediationCheck.code }}</code>
        </p>
      </div>
      <template #actions>
        <Button variant="ghost" @click="closeRemediation">稍后处理</Button>
        <a
          v-if="guidanceFor(remediationCheck)"
          :data-testid="guidanceFor(remediationCheck)?.testId"
          class="check-action-link"
          :href="guidanceFor(remediationCheck)?.href"
          target="_blank"
          rel="noreferrer"
        >
          {{ guidanceFor(remediationCheck)?.label }}
        </a>
        <Button
          v-if="canStartLogin(remediationCheck)"
          data-testid="runtime-login"
          variant="primary"
          @click="loginFromRemediation"
        >
          重新登录并修复认证
        </Button>
        <Button
          v-if="remediationCheck.id === 'authentication' || remediationCheck.id === 'acp'"
          :data-testid="`runtime-remediation-retry-${remediationCheck.id}`"
          :variant="canStartLogin(remediationCheck) ? 'secondary' : 'primary'"
          @click="retryFromRemediation"
        >
          重启 ACP 并复检
        </Button>
        <Button
          v-else-if="!canStartLogin(remediationCheck)"
          :data-testid="`runtime-remediation-retry-${remediationCheck.id}`"
          variant="primary"
          @click="retryFromRemediation"
        >
          立即复检
        </Button>
      </template>
    </Dialog>
  </section>
</template>

<style scoped>
.onboarding { width:min(840px, calc(100vw - 48px)); margin:48px auto; padding:var(--space-6); color:var(--ctp-text); background:var(--ctp-mantle); border:1px solid var(--ctp-surface0); border-radius:var(--radius-dialog); }
.section-heading { display:flex; align-items:flex-start; justify-content:space-between; gap:var(--space-6); }
h1 { margin:0; font-size:var(--text-4xl); line-height:var(--leading-tight); font-weight:var(--font-weight-semibold); }
.intro { margin:var(--space-2) 0 0; color:var(--ctp-subtext0); }
.overall { padding:6px 10px; color:var(--ctp-blue); border:1px solid var(--ctp-surface1); border-radius:999px; font-size:var(--font-small); white-space:nowrap; }
.check-list { display:grid; gap:var(--space-2); margin:var(--space-6) 0; padding:0; list-style:none; }
.check-row { display:grid; grid-template-columns:150px minmax(0, 1fr) auto; align-items:center; gap:var(--space-4); padding:var(--space-3) var(--space-4); background:var(--ctp-base); border:1px solid var(--ctp-surface0); border-radius:var(--radius-card); }
.check-row.is-unresolved { border-color:var(--ctp-surface1); }
.check-copy strong { display:block; margin-bottom:2px; }
.check-copy p, .login-state p { margin:0; color:var(--ctp-subtext0); }
.check-actions { display:flex; align-items:center; justify-content:flex-end; flex-wrap:wrap; gap:var(--space-2); }
.check-action-link { min-height:var(--button-height); display:inline-flex; align-items:center; padding:0 var(--space-3); color:var(--ctp-crust); background:var(--ctp-mauve); border:1px solid var(--ctp-mauve); border-radius:var(--radius-control); text-decoration:none; white-space:nowrap; }
.check-action-link:hover { filter:brightness(1.06); }
.check-action-link:focus-visible { outline:2px solid var(--ctp-mauve); outline-offset:2px; }
.remediation-page { display:grid; gap:var(--space-4); }
.remediation-page p { margin:0; }
.repair-summary { padding:var(--space-3); color:var(--ctp-text); background:var(--ctp-base); border:1px solid var(--ctp-surface1); border-radius:var(--radius-card); }
.error-code { color:var(--ctp-subtext0); }
.error-code code { color:var(--ctp-text); }
.login-state { display:flex; align-items:center; justify-content:space-between; gap:var(--space-4); padding:var(--space-4); border:1px solid var(--ctp-blue); border-radius:var(--radius-card); }
.login-result { color:var(--ctp-subtext0); }
.bridge-error { color:var(--ctp-red); }
.actions { display:flex; align-items:center; flex-wrap:wrap; gap:var(--space-3); }
@media (max-width:720px) { .onboarding { width:calc(100vw - 24px); margin:12px auto; padding:var(--space-4); }.section-heading { flex-direction:column; }.check-row { grid-template-columns:1fr; gap:var(--space-2); }.check-actions { justify-content:flex-start; } }
</style>
