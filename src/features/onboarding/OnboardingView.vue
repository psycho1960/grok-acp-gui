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
import StatusIcon from "../../shared/ui/StatusIcon.vue";

const props = defineProps<{ bridge: DesktopBridge }>();
const emit = defineEmits<{ ready: [snapshot: RuntimeReadinessSnapshot] }>();

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
const bridgeError = ref<string | null>(null);
const copied = ref(false);
let pollTimer: ReturnType<typeof setTimeout> | undefined;

const isLoginRunning = computed(() => login.value.status === "running");
const needsLogin = computed(
  () =>
    snapshot.value?.authenticated === false ||
    checks.value.some((check) => check.code === "RUNTIME_LOGIN_FAILED"),
);
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

async function refresh(): Promise<void> {
  loading.value = true;
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

async function copyDiagnostic(): Promise<void> {
  const diagnostic = snapshot.value?.actionableError?.diagnostic;
  if (!diagnostic) return;
  try {
    await navigator.clipboard.writeText(diagnostic);
    copied.value = true;
  } catch {
    bridgeError.value = "无法访问剪贴板。";
  }
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
      <li v-for="check in checks" :key="check.id" class="check-row" :data-check-id="check.id">
        <StatusIcon :status="iconStatus(check.status)" :label="`${check.label}：${check.status}`" />
        <div class="check-copy">
          <strong>{{ check.label }}</strong>
          <p>{{ check.detail }}</p>
          <p v-if="check.action" class="action-copy">{{ check.action }}</p>
        </div>
      </li>
    </ol>

    <div v-if="isLoginRunning" class="login-state" role="status" aria-live="polite">
      <div>
        <strong>Grok 登录进程正在运行</strong>
        <p>{{ login.message ?? "请在浏览器中完成官方登录流程。" }}</p>
      </div>
      <Button data-testid="runtime-login-cancel" variant="ghost" @click="cancelLogin">取消登录</Button>
    </div>
    <p v-else-if="login.status !== 'idle'" class="login-result" role="status">
      {{ login.message }}<span v-if="login.exitCode !== undefined">（退出码 {{ login.exitCode }}）</span>
    </p>

    <p v-if="bridgeError" class="bridge-error" role="alert">{{ bridgeError }}</p>

    <footer class="actions">
      <Button data-testid="runtime-refresh" :state="loading ? 'loading' : 'default'" @click="refresh">重新检测</Button>
      <Button v-if="needsLogin && !isLoginRunning" data-testid="runtime-login" variant="primary" @click="startLogin">登录 Grok</Button>
      <a
        v-if="snapshot && !snapshot.installed"
        data-testid="grok-install-link"
        class="install-link"
        href="https://docs.x.ai/build/getting-started"
        target="_blank"
        rel="noreferrer"
      >查看官方安装说明</a>
      <Button v-if="snapshot?.actionableError" variant="ghost" @click="copyDiagnostic">
        {{ copied ? "已复制脱敏诊断" : "复制脱敏诊断" }}
      </Button>
    </footer>
  </section>
</template>

<style scoped>
.onboarding { width:min(840px, calc(100vw - 48px)); margin:48px auto; padding:var(--space-6); color:var(--ctp-text); background:var(--ctp-mantle); border:1px solid var(--ctp-surface0); border-radius:var(--radius-dialog); }
.section-heading { display:flex; align-items:flex-start; justify-content:space-between; gap:var(--space-6); }
h1 { margin:0; font-size:28px; }
.intro { margin:var(--space-2) 0 0; color:var(--ctp-subtext0); }
.overall { padding:6px 10px; color:var(--ctp-blue); border:1px solid var(--ctp-surface1); border-radius:999px; font-size:var(--font-small); white-space:nowrap; }
.check-list { display:grid; gap:var(--space-2); margin:var(--space-6) 0; padding:0; list-style:none; }
.check-row { display:grid; grid-template-columns:150px minmax(0, 1fr); gap:var(--space-4); padding:var(--space-3) var(--space-4); background:var(--ctp-base); border:1px solid var(--ctp-surface0); border-radius:var(--radius-card); }
.check-copy strong { display:block; margin-bottom:2px; }
.check-copy p, .login-state p { margin:0; color:var(--ctp-subtext0); }
.check-copy .action-copy { margin-top:4px; color:var(--ctp-yellow); }
.login-state { display:flex; align-items:center; justify-content:space-between; gap:var(--space-4); padding:var(--space-4); border:1px solid var(--ctp-blue); border-radius:var(--radius-card); }
.login-result { color:var(--ctp-subtext0); }
.bridge-error { color:var(--ctp-red); }
.actions { display:flex; align-items:center; flex-wrap:wrap; gap:var(--space-3); }
.install-link { min-height:var(--button-height); display:inline-flex; align-items:center; color:var(--ctp-blue); }
@media (max-width:720px) { .onboarding { width:calc(100vw - 24px); margin:12px auto; padding:var(--space-4); }.section-heading { flex-direction:column; }.check-row { grid-template-columns:1fr; gap:var(--space-2); } }
</style>
