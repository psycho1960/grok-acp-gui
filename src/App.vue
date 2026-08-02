<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  bootstrap,
  loadPreferences,
  savePreferences,
  selectProjectDirectory,
  type BootstrapStatus,
} from "./bridge/desktop-bridge";

const isLoading = ref(true);
const startupError = ref<string | null>(null);
const bootstrapStatus = ref<BootstrapStatus | null>(null);
const projectPath = ref("");

onMounted(async () => {
  try {
    const [status, preferences] = await Promise.all([
      bootstrap(),
      loadPreferences(),
    ]);
    bootstrapStatus.value = status;
    projectPath.value = preferences.projectPath ?? "";
  } catch (error) {
    startupError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isLoading.value = false;
  }
});

async function chooseProjectDirectory() {
  try {
    const selected = await selectProjectDirectory();
    if (selected) {
      projectPath.value = selected;
      await savePreferences({ projectPath: selected });
    }
  } catch (error) {
    startupError.value = error instanceof Error ? error.message : String(error);
  }
}

async function persistProjectPath() {
  await savePreferences({ projectPath: projectPath.value.trim() || null });
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

    <section v-else class="workspace-card">
      <div class="section-heading">
        <div>
          <p class="eyebrow">UI-ONBOARD-001</p>
          <h2>选择项目目录</h2>
        </div>
        <span class="status-pill" role="status">
          <span class="status-dot" aria-hidden="true"></span>
          工程壳就绪
        </span>
      </div>

      <p class="intro">
        这是 Grok ACP GUI 的可启动基线。项目、任务、会话和审批能力将在后续任务中接入。
      </p>

      <label class="field-label" for="project-path">本地 Git 项目</label>
      <div class="project-picker">
        <input
          id="project-path"
          v-model="projectPath"
          type="text"
          placeholder="选择或输入项目目录"
          autocomplete="off"
          spellcheck="false"
          @change="persistProjectPath"
        />
        <button type="button" class="primary-button" @click="chooseProjectDirectory">
          选择目录
        </button>
      </div>

      <div v-if="!projectPath" class="empty-state">
        <span class="empty-icon" aria-hidden="true">⌂</span>
        <div>
          <strong>尚未选择项目</strong>
          <p>选择一个本地目录后，后续任务可以在此基础上接入。</p>
        </div>
      </div>
      <div v-else class="selected-state">
        <span class="selected-icon" aria-hidden="true">✓</span>
        <div>
          <strong>项目目录已保存</strong>
          <p>{{ projectPath }}</p>
        </div>
      </div>
    </section>

    <footer class="statusbar">
      <span>ACP transport 基础已保留</span>
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

.brand,
.section-heading,
.project-picker,
.empty-state,
.selected-state {
  display: flex;
  align-items: center;
}

.brand {
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

.version-badge,
.status-pill {
  padding: 6px 10px;
  border: 1px solid var(--ctp-surface1);
  border-radius: 999px;
  color: var(--ctp-subtext0);
  font-size: 12px;
}

.workspace-card,
.state-card {
  width: min(760px, calc(100% - 48px));
  margin: auto;
  padding: 32px;
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface0);
  border-radius: 14px;
  box-shadow: 0 20px 60px rgb(0 0 0 / 18%);
}

.state-card {
  text-align: center;
}

.error-card {
  border-color: var(--ctp-red);
}

.section-heading {
  justify-content: space-between;
  gap: 20px;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--ctp-green);
  border-color: var(--ctp-green);
}

.status-dot {
  width: 7px;
  height: 7px;
  background: var(--ctp-green);
  border-radius: 50%;
}

.intro {
  max-width: 620px;
  margin: 18px 0 30px;
  color: var(--ctp-subtext0);
  line-height: 1.7;
}

.field-label {
  display: block;
  margin-bottom: 8px;
  color: var(--ctp-text);
  font-size: 13px;
  font-weight: 600;
}

.project-picker {
  gap: 10px;
}

input {
  min-width: 0;
  flex: 1;
  height: 40px;
  padding: 0 12px;
  color: var(--ctp-text);
  background: var(--ctp-base);
  border: 1px solid var(--ctp-surface1);
  border-radius: 7px;
  outline: none;
}

input:focus {
  border-color: var(--ctp-mauve);
  box-shadow: 0 0 0 3px rgb(203 166 247 / 18%);
}

.primary-button {
  height: 40px;
  padding: 0 16px;
  color: var(--ctp-crust);
  background: var(--ctp-mauve);
  border: 0;
  border-radius: 7px;
  cursor: pointer;
  font-weight: 700;
}

.primary-button:hover {
  background: var(--ctp-lavender);
}

.empty-state,
.selected-state {
  gap: 12px;
  margin-top: 22px;
  padding: 16px;
  border: 1px dashed var(--ctp-surface1);
  border-radius: 9px;
}

.selected-state {
  border-style: solid;
  border-color: var(--ctp-green);
}

.empty-icon,
.selected-icon,
.state-icon {
  display: grid;
  flex: 0 0 auto;
  width: 34px;
  height: 34px;
  place-items: center;
  color: var(--ctp-blue);
  background: var(--ctp-surface0);
  border-radius: 50%;
  font-weight: 800;
}

.selected-icon {
  color: var(--ctp-green);
}

.state-icon {
  margin: 0 auto 16px;
  color: var(--ctp-yellow);
  font-size: 20px;
}

.loading-icon {
  color: var(--ctp-blue);
  animation: pulse 1.2s ease-in-out infinite;
}

.empty-state p,
.selected-state p,
.muted,
.state-card p {
  margin: 5px 0 0;
  color: var(--ctp-subtext0);
  line-height: 1.55;
}

.selected-state p {
  overflow-wrap: anywhere;
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

  .workspace-card,
  .state-card {
    width: calc(100% - 32px);
    padding: 24px;
  }

  .section-heading {
    align-items: flex-start;
    flex-direction: column;
  }

  .project-picker {
    align-items: stretch;
    flex-direction: column;
  }

  .statusbar {
    gap: 8px;
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
