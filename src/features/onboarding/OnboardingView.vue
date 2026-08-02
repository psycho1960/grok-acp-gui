<script setup lang="ts">
defineProps<{
  error: string | null;
  projectPath: string;
}>();

const emit = defineEmits<{
  "select-directory": [];
  "update:projectPath": [value: string];
}>();

function updateProjectPath(event: globalThis.Event) {
  emit(
    "update:projectPath",
    (event.target as globalThis.HTMLInputElement).value,
  );
}
</script>

<template>
  <section class="workspace-card surface-card" aria-labelledby="onboarding-title">
    <div class="section-heading">
      <div>
        <p class="eyebrow">UI-ONBOARD-001</p>
        <h2 id="onboarding-title">选择项目目录</h2>
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
        :value="projectPath"
        type="text"
        placeholder="选择或输入项目目录"
        autocomplete="off"
        spellcheck="false"
        @input="updateProjectPath"
      />
      <button type="button" class="primary-button" @click="emit('select-directory')">
        选择目录
      </button>
    </div>

    <p v-if="error" class="selection-error" role="alert">{{ error }}</p>

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
        <strong>项目目录已选择</strong>
        <p>{{ projectPath }}</p>
        <small>仅保留在本次运行中，不写入本地持久化存储。</small>
      </div>
    </div>
  </section>
</template>

<style scoped>
.workspace-card {
}

.section-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}

.eyebrow {
  margin: 0 0 3px;
  color: var(--ctp-subtext0);
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

h2,
p {
  margin-top: 0;
}

h2 {
  margin-bottom: 0;
  color: var(--ctp-text);
  font-size: 24px;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 6px 10px;
  color: var(--ctp-green);
  border: 1px solid var(--ctp-green);
  border-radius: 999px;
  font-size: 12px;
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
  display: flex;
  align-items: center;
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

.selection-error {
  margin: 12px 0 0;
  color: var(--ctp-red);
  line-height: 1.55;
}

.empty-state,
.selected-state {
  display: flex;
  align-items: center;
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
.selected-icon {
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

.empty-state p,
.selected-state p,
.selected-state small {
  display: block;
  margin: 5px 0 0;
  color: var(--ctp-subtext0);
  line-height: 1.55;
}

.selected-state p {
  overflow-wrap: anywhere;
}

@media (max-width: 720px) {
  .section-heading {
    align-items: flex-start;
    flex-direction: column;
  }

  .project-picker {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
