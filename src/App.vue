<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  bootstrap,
  selectProjectDirectory,
  type BootstrapStatus,
} from "./bridge/desktop-bridge";
import OnboardingView from "./features/onboarding/OnboardingView.vue";

const isLoading = ref(true);
const startupError = ref<string | null>(null);
const selectionError = ref<string | null>(null);
const bootstrapStatus = ref<BootstrapStatus | null>(null);
const projectPath = ref("");

onMounted(async () => {
  try {
    bootstrapStatus.value = await bootstrap();
  } catch (error) {
    startupError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isLoading.value = false;
  }
});

async function chooseProjectDirectory() {
  selectionError.value = null;
  try {
    const selected = await selectProjectDirectory();
    if (selected) projectPath.value = selected;
  } catch (error) {
    selectionError.value = error instanceof Error ? error.message : String(error);
  }
}
</script>

<template>
  <main class="shell" aria-labelledby="app-title">
    <header class="topbar">
      <div class="brand" aria-label="Grok ACP GUI">
        <span class="brand-mark" aria-hidden="true">G</span>
        <div>
          <p class="eyebrow">Windows desktop</p>
          <h1 id="app-title">Grok ACP GUI</h1>
        </div>
      </div>
      <span v-if="bootstrapStatus" class="version-badge">
        v{{ bootstrapStatus.version }}
      </span>
    </header>

    <section v-if="isLoading" class="state-card" aria-live="polite">
      <div class="state-icon loading-icon" aria-hidden="true">···</div>
      <h2>正在启动</h2>
      <p>正在准备桌面工程壳。</p>
    </section>

    <section v-else-if="startupError" class="state-card error-card" role="alert">
      <div class="state-icon" aria-hidden="true">!</div>
      <h2>启动失败</h2>
      <p>{{ startupError }}</p>
      <p class="muted">请关闭窗口后重试；详细诊断将在后续任务中接入。</p>
    </section>

    <OnboardingView
      v-else
      :error="selectionError"
      :project-path="projectPath"
      @select-directory="chooseProjectDirectory"
      @update:project-path="projectPath = $event"
    />

    <footer class="statusbar">
      <span>ACP SDK 已保留 · 传输接入待后续任务</span>
      <span>GAG-001 · 工程基线</span>
    </footer>
  </main>
</template>

<style scoped>
.shell {
  min-height: 100vh;
  display: grid;
  grid-template-rows: auto 1fr auto;
  color: var(--ctp-text);
  background: var(--ctp-base);
}

.topbar,
.statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 18px 28px;
  background: var(--ctp-mantle);
  border-bottom: 1px solid var(--ctp-surface0);
}

.statusbar {
  padding: 10px 28px;
  color: var(--ctp-subtext0);
  font-size: 12px;
  border-top: 1px solid var(--ctp-surface0);
  border-bottom: 0;
}

.brand {
  display: flex;
  align-items: center;
  gap: 12px;
}

.brand-mark {
  display: grid;
  width: 38px;
  height: 38px;
  place-items: center;
  color: var(--ctp-crust);
  background: var(--ctp-mauve);
  border-radius: 10px;
  font-weight: 800;
  font-size: 20px;
}

.eyebrow {
  margin: 0 0 3px;
  color: var(--ctp-subtext0);
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

h1,
h2,
p {
  margin-top: 0;
}

h1 {
  margin-bottom: 0;
  font-size: 18px;
}

h2 {
  margin-bottom: 0;
  font-size: 24px;
}

.version-badge {
  padding: 6px 10px;
  color: var(--ctp-subtext0);
  border: 1px solid var(--ctp-surface1);
  border-radius: 999px;
  font-size: 12px;
}

.state-card {
  width: min(760px, calc(100% - 48px));
  margin: auto;
  padding: 32px;
  text-align: center;
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface0);
  border-radius: 14px;
  box-shadow: 0 20px 60px rgb(0 0 0 / 18%);
}

.error-card {
  border-color: var(--ctp-red);
}

.state-icon {
  display: grid;
  width: 34px;
  height: 34px;
  margin: 0 auto 16px;
  place-items: center;
  color: var(--ctp-yellow);
  background: var(--ctp-surface0);
  border-radius: 50%;
  font-size: 20px;
  font-weight: 800;
}

.loading-icon {
  color: var(--ctp-blue);
  animation: pulse 1.2s ease-in-out infinite;
}

.muted,
.state-card p {
  margin: 5px 0 0;
  color: var(--ctp-subtext0);
  line-height: 1.55;
}

@keyframes pulse {
  50% {
    opacity: 0.45;
  }
}

@media (max-width: 720px) {
  .topbar,
  .statusbar {
    padding-inline: 18px;
  }

  .state-card {
    width: calc(100% - 32px);
    padding: 24px;
  }

  .statusbar {
    gap: 8px;
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
