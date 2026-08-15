<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import IconButton from "../../shared/ui/IconButton.vue";
import NamedIcon from "../../shared/ui/NamedIcon.vue";
import Select from "../../shared/ui/Select.vue";
import type { ModelInfo, ReasoningEffort, SlashCommandInfo } from "../../bridge/types";
import { extractImageFiles } from "./clipboard-images";
import {
  filterSlashCommands,
  insertSlashCommand,
  slashMenuState,
} from "./slash-commands";
import type { ComposerAttachment, ComposerCapabilities } from "./types";

const props = defineProps<{
  modelValue: string;
  capabilities: ComposerCapabilities;
  sendError?: string | null;
  sendPending?: boolean;
  attachmentPending?: boolean;
  attachments?: ComposerAttachment[];
  /** Native (OS-level) drag-over highlight driven by the parent view. */
  dropActive?: boolean;
  /** grok build quick commands discovered from ACP available_commands. */
  slashCommands?: SlashCommandInfo[];
  slashCommandsPending?: boolean;
  models?: ModelInfo[];
  selectedModel?: string | null;
  selectedReasoning?: ReasoningEffort | null;
  settingsLocked?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  send: [];
  queue: [];
  cancel: [];
  addAttachments: [];
  dropAttachments: [paths: string[]];
  pasteImages: [files: File[]];
  removeAttachment: [artifactId: string];
  "update:model": [model: string | null];
  "update:reasoning": [reasoning: ReasoningEffort];
  "request-slash-commands": [];
}>();

const textarea = ref<HTMLTextAreaElement | null>(null);
const modelMenuOpen = ref(false);
const hoverDropActive = ref(false);
const slashOpen = ref(false);
const slashHelpPinned = ref(false);
const slashQuery = ref("");
const slashLineStart = ref(0);
const slashIndex = ref(0);
/** Suppress slash-menu syncs until the next real input event (an Esc close
 *  or a selection must not be undone by the keyup that follows it). */
let slashEscapeLock = false;
let slashRequestIssued = false;
const hasContent = computed(() => props.modelValue.trim().length > 0 || (props.attachments?.length ?? 0) > 0);

const REASONING_LABEL: Record<string, string> = {
  low: "低",
  medium: "中",
  high: "高",
  max: "最高",
};

const modelOptions = computed(() => [
  { value: "", label: "使用运行时默认模型" },
  ...(props.models ?? [])
    .filter((model) => model.modelId.trim().length > 0)
    .map((model) => ({ value: model.modelId, label: model.name || model.modelId })),
]);

const reasoningOptions = [
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "max", label: "最高" },
];

const modelSummary = computed(() => {
  const model =
    modelOptions.value.find((option) => option.value === (props.selectedModel ?? ""))?.label ??
    "默认模型";
  const reasoning = REASONING_LABEL[props.selectedReasoning ?? "medium"] ?? "中";
  return `${model} · ${reasoning}`;
});

const filteredCommands = computed(() =>
  filterSlashCommands(props.slashCommands ?? [], slashHelpPinned.value ? "" : slashQuery.value),
);

const showSlashMenu = computed(
  () => slashOpen.value || slashHelpPinned.value,
);

function requestSlashCommandsIfNeeded(): void {
  if (
    slashRequestIssued ||
    props.slashCommandsPending ||
    (props.slashCommands?.length ?? 0) > 0
  ) return;
  slashRequestIssued = true;
  emit("request-slash-commands");
}

function syncSlashMenu(): void {
  if (slashEscapeLock) return;
  const el = textarea.value;
  if (!el) return;
  const state = slashMenuState(el.value, el.selectionStart);
  slashOpen.value = state.open;
  if (state.open) modelMenuOpen.value = false;
  slashQuery.value = state.query;
  slashLineStart.value = state.lineStart;
  if (slashIndex.value >= filteredCommands.value.length) {
    slashIndex.value = 0;
  }
  if (state.open) {
    requestSlashCommandsIfNeeded();
    void nextTick(scrollSlashItemIntoView);
  }
}

function closeSlashMenu(): void {
  slashOpen.value = false;
  slashHelpPinned.value = false;
  slashQuery.value = "";
  slashEscapeLock = true;
  slashRequestIssued = false;
}

function openSlashHelp(): void {
  modelMenuOpen.value = false;
  slashHelpPinned.value = true;
  slashOpen.value = true;
  slashQuery.value = "";
  slashIndex.value = 0;
  requestSlashCommandsIfNeeded();
}

function scrollSlashItemIntoView(): void {
  document
    .querySelector('[data-testid="slash-menu-item"].active')
    ?.scrollIntoView({ block: "nearest" });
}

function applySlashCommand(command: SlashCommandInfo): void {
  const el = textarea.value;
  if (!el) return;
  // Help button can open without a leading "/"; only replace when the caret is
  // already on a "/..." token so plain draft text is never wiped.
  const onSlashLine = slashMenuState(el.value, el.selectionStart).open;
  const mode = onSlashLine ? "replace-slash-line" : "insert-at-cursor";
  const { text, cursor } = insertSlashCommand(
    el.value,
    el.selectionStart,
    command.name,
    mode,
  );
  el.value = text;
  el.setSelectionRange(cursor, cursor);
  emit("update:modelValue", text);
  closeSlashMenu();
  void nextTick(() => el.focus());
}

function onKeydown(event: KeyboardEvent): void {
  if (showSlashMenu.value) {
    const commands = filteredCommands.value;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      slashIndex.value = commands.length ? (slashIndex.value + 1) % commands.length : 0;
      void nextTick(scrollSlashItemIntoView);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      slashIndex.value = commands.length
        ? (slashIndex.value - 1 + commands.length) % commands.length
        : 0;
      void nextTick(scrollSlashItemIntoView);
      return;
    }
    if (event.key === "Enter") {
      const command = commands[slashIndex.value];
      if (command) {
        event.preventDefault();
        applySlashCommand(command);
        return;
      }
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeSlashMenu();
      return;
    }
    if (event.key === "Tab" && commands.length > 0) {
      event.preventDefault();
      applySlashCommand(commands[slashIndex.value] ?? commands[0]);
      return;
    }
  }
  if (event.key === "Escape") {
    if (props.capabilities.canCancel) {
      event.preventDefault();
      emit("cancel");
    }
    return;
  }
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    if (props.capabilities.canSend && hasContent.value) {
      emit("send");
      return;
    }
    if (!props.capabilities.canSend && props.capabilities.canCancel && hasContent.value) {
      emit("queue");
    }
  }
}

function onInput(event: Event): void {
  slashEscapeLock = false;
  const value = (event.target as HTMLTextAreaElement).value;
  emit("update:modelValue", value);
  void nextTick(syncSlashMenu);
}

function onSelectionChange(): void {
  if (slashOpen.value) syncSlashMenu();
}

function pathsFromTransfer(transfer: DataTransfer | null): string[] {
  return Array.from(transfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path)
    .filter((path): path is string => typeof path === "string" && path.length > 0);
}

function onPaste(event: ClipboardEvent): void {
  // 1) Clipboard images (Win+Shift+S): blob items with no filesystem path.
  const images = extractImageFiles(event.clipboardData);
  if (images.length > 0) {
    event.preventDefault();
    emit("pasteImages", images.map((image) => image.file));
    return;
  }
  // 2) Path-backed files (older drag sources) reuse the drop pipeline.
  const paths = pathsFromTransfer(event.clipboardData);
  if (paths.length) {
    event.preventDefault();
    emit("dropAttachments", paths);
  }
}

function onDrop(event: DragEvent): void {
  event.preventDefault();
  hoverDropActive.value = false;
  const paths = pathsFromTransfer(event.dataTransfer);
  if (paths.length) emit("dropAttachments", paths);
}

function onModelChange(value: string): void {
  emit("update:model", value === "" ? null : value);
}

function onReasoningChange(value: string): void {
  if (value === "low" || value === "medium" || value === "high" || value === "max") {
    emit("update:reasoning", value);
  }
}

function toggleModelMenu(): void {
  if (props.settingsLocked) return;
  const opening = !modelMenuOpen.value;
  modelMenuOpen.value = opening;
  if (opening) closeSlashMenu();
}

function pickModel(value: string): void {
  onModelChange(value);
}

function pickReasoning(value: string): void {
  onReasoningChange(value);
}

defineExpose({ textarea, focus: () => textarea.value?.focus() });
</script>

<template>
  <footer
    class="composer"
    :class="{ 'drop-active': hoverDropActive || props.dropActive }"
    data-testid="composer"
    aria-label="消息输入"
    @dragover.prevent="hoverDropActive = true"
    @dragleave="hoverDropActive = false"
    @drop="onDrop"
  >
    <div v-if="hoverDropActive || props.dropActive" class="drop-zone" aria-hidden="true">松开即可添加图片</div>
    <p v-if="capabilities.disabledReason" class="disabled-reason" role="status">
      {{ capabilities.disabledReason }}
    </p>
    <ul v-if="attachments?.length" class="attachment-list" aria-label="待发送附件">
      <li v-for="attachment in attachments" :key="attachment.artifactId">
        <span aria-hidden="true">▧</span><span>{{ attachment.displayName }}</span>
        <span class="attachment-size">{{ Math.ceil(attachment.bytes / 1024) }} KiB</span>
        <button type="button" :aria-label="`移除附件 ${attachment.displayName}`" :disabled="sendPending" @click="emit('removeAttachment', attachment.artifactId)">×</button>
      </li>
    </ul>
    <p v-if="sendError" class="send-error" role="alert" data-testid="send-error">
      {{ sendError }}
    </p>
    <div class="row">
      <div class="composer-input-wrap">
        <div
          v-if="showSlashMenu"
          class="slash-menu"
          data-testid="slash-menu"
          role="listbox"
          aria-label="快捷指令"
        >
          <p v-if="filteredCommands.length === 0" class="slash-empty" role="status">
            {{
              slashCommandsPending
                ? "正在获取快捷指令…"
                : (slashCommands?.length ?? 0) === 0
                  ? "暂无可用快捷指令"
                  : "没有匹配的快捷指令"
            }}
          </p>
          <button
            v-for="(command, index) in filteredCommands"
            :key="command.name"
            type="button"
            class="slash-item"
            :class="{ active: index === slashIndex }"
            data-testid="slash-menu-item"
            role="option"
            :aria-selected="index === slashIndex"
            @mouseenter="slashIndex = index"
            @click="applySlashCommand(command)"
          >
            <span class="slash-name">/{{ command.name }}</span>
            <span class="slash-desc">{{ command.description || "（无描述）" }}</span>
          </button>
        </div>
        <label class="sr-only" for="composer-input">消息</label>
        <textarea
          id="composer-input"
          ref="textarea"
          class="input"
          data-testid="composer-input"
          rows="3"
          :value="modelValue"
          :disabled="!capabilities.canSend && !capabilities.canCancel"
          :placeholder="
            capabilities.bridgeOnline
              ? '输入消息…'
              : 'Bridge 离线 — 草稿仍会保留'
          "
          @input="onInput"
          @keydown="onKeydown"
          @paste="onPaste"
          @keyup="syncSlashMenu"
          @click="onSelectionChange"
          @select="onSelectionChange"
        />
        <div class="dock-tools">
          <IconButton
            label="添加图片"
            data-testid="composer-add-attachment"
            :disabled="!capabilities.canSend || sendPending"
            :state="attachmentPending ? 'loading' : 'default'"
            @click="emit('addAttachments')"
          >
            <NamedIcon name="paperclip" :size="16" />
          </IconButton>
          <IconButton
            label="/ 指令"
            data-testid="composer-slash-help"
            :disabled="!capabilities.canSend"
            @click="openSlashHelp"
          >
            <span class="slash-glyph">/</span>
          </IconButton>
          <div
            class="model-control"
            :class="{ locked: settingsLocked }"
            data-testid="composer-model-control"
          >
            <button
              v-if="!settingsLocked"
              type="button"
              class="model-toggle"
              data-testid="model-reasoning-toggle"
              @click="toggleModelMenu"
            >
              <span class="model-summary">{{ modelSummary }}</span>
              <NamedIcon name="chevronDown" :size="12" data-testid="model-chevron" />
            </button>
            <span v-else class="model-summary">{{ modelSummary }}</span>
            <div
              v-if="modelMenuOpen && !settingsLocked"
              class="model-menu"
              data-testid="model-reasoning-menu"
            >
              <p class="menu-label">模型</p>
              <button
                v-for="option in modelOptions"
                :key="`model-${option.value}`"
                type="button"
                class="menu-item"
                :class="{ on: (selectedModel ?? '') === option.value }"
                @click="pickModel(option.value)"
              >
                {{ option.label }}
              </button>
              <p class="menu-label">推理</p>
              <button
                v-for="option in reasoningOptions"
                :key="`reason-${option.value}`"
                type="button"
                class="menu-item"
                :class="{ on: (selectedReasoning ?? 'medium') === option.value }"
                @click="pickReasoning(option.value)"
              >
                {{ option.label }}
              </button>
            </div>
            <div class="is-visually-hidden" aria-hidden="true">
              <Select
                class="settings-select"
                data-testid="conversation-model-select"
                label="模型"
                :model-value="selectedModel ?? ''"
                :options="modelOptions"
                :disabled="settingsLocked"
                tabindex="-1"
                @update:model-value="onModelChange"
              />
              <Select
                class="settings-select"
                data-testid="conversation-reasoning-select"
                label="推理强度"
                :model-value="selectedReasoning ?? 'medium'"
                :options="reasoningOptions"
                :disabled="settingsLocked"
                tabindex="-1"
                @update:model-value="onReasoningChange"
              />
            </div>
          </div>
        </div>
      </div>
      <div class="actions">
        <button
          v-if="capabilities.canCancel"
          type="button"
          class="dock-circle is-stop"
          data-testid="composer-stop"
          aria-label="停止"
          :disabled="sendPending"
          @click="emit('cancel')"
        >
          <NamedIcon name="x" :size="16" />
        </button>
        <button
          v-else
          type="button"
          class="dock-circle is-send"
          data-testid="composer-send"
          aria-label="发送"
          :disabled="!capabilities.canSend || !hasContent"
          @click="emit('send')"
        >
          <NamedIcon name="play" :size="16" />
        </button>
      </div>
    </div>
  </footer>
</template>

<style scoped>
.composer {
  position: relative;
  flex-shrink: 0;
  margin: 0 var(--space-3) var(--space-3);
  padding: var(--space-3);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: 20px;
}
.composer.drop-active { outline: 2px solid var(--ctp-blue); outline-offset: -2px; }
.drop-zone { position: absolute; inset: 0; z-index: 2; display: grid; place-items: center; color: var(--ctp-text); background: color-mix(in srgb, var(--ctp-mantle) 90%, transparent); border: 2px dashed var(--ctp-blue); pointer-events: none; }
.attachment-list { display: flex; flex-wrap: wrap; gap: var(--space-2); padding: 0; margin: 0 0 var(--space-2); list-style: none; }
.attachment-list li { display: inline-flex; max-width: 100%; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; background: var(--ctp-surface0); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); }
.attachment-list button { color: var(--ctp-subtext0); cursor: pointer; background: transparent; border: 0; }
.attachment-list button:focus-visible { outline: 2px solid var(--ctp-blue); outline-offset: 2px; }
.attachment-size { color: var(--ctp-subtext0); font-size: var(--font-small); }
.row {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: var(--space-2);
  align-items: end;
}
.composer-input-wrap {
  position: relative;
  display: grid;
  gap: var(--space-1);
}
.slash-menu {
  position: absolute;
  bottom: calc(100% + 4px);
  left: 0;
  z-index: 30;
  display: grid;
  gap: 2px;
  width: min(420px, 100%);
  max-height: 240px;
  padding: var(--space-1);
  overflow-y: auto;
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  box-shadow: var(--elevation-menu);
}
.slash-item {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: var(--space-2);
  align-items: center;
  padding: var(--space-1) var(--space-2);
  text-align: left;
  color: var(--ctp-text);
  cursor: pointer;
  background: transparent;
  border: 0;
  border-radius: calc(var(--radius-control) - 2px);
}
.slash-item:hover,
.slash-item.active {
  background: var(--overlay-menu-active);
}
.slash-name {
  font-weight: 600;
  color: var(--ctp-blue);
}
.slash-desc {
  overflow: hidden;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.slash-empty {
  margin: 0;
  padding: var(--space-2);
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.input {
  width: 100%;
  min-height: 56px;
  max-height: 200px;
  padding: var(--space-2);
  resize: vertical;
  color: var(--ctp-text);
  background: transparent;
  border: 0;
}
.dock-tools {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1);
  align-items: center;
}
.slash-glyph {
  font-weight: var(--font-weight-semibold);
}
.model-control {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  min-height: var(--control-min-size);
  padding: 0 var(--space-2);
  border: 1px solid var(--ctp-surface1);
  border-radius: 999px;
}
.model-toggle {
  display: inline-flex;
  gap: var(--space-1);
  align-items: center;
  padding: 0;
  color: inherit;
  background: transparent;
  border: 0;
  cursor: pointer;
}
.model-menu {
  position: absolute;
  bottom: calc(100% + 4px);
  left: 0;
  z-index: 5;
  display: grid;
  gap: 2px;
  min-width: 180px;
  padding: var(--space-1);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  box-shadow: var(--shadow-md);
}
.menu-label {
  margin: var(--space-1) var(--space-2) 0;
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.menu-item {
  padding: 6px 8px;
  color: var(--ctp-text);
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: var(--radius-control);
  cursor: pointer;
}
.menu-item.on,
.menu-item:hover {
  background: var(--overlay-hover);
}
.model-control.locked {
  color: var(--ctp-subtext0);
}
.model-summary {
  font-size: var(--font-small);
  white-space: nowrap;
}
.model-control :deep(.field > span) {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
.model-control :deep(select) {
  min-height: 28px;
  max-width: 92px;
  padding: 0;
  color: inherit;
  background: transparent;
  border: 0;
  appearance: none;
}
.model-control .is-visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
.dock-circle {
  display: grid;
  width: 36px;
  height: 36px;
  place-items: center;
  color: var(--ctp-crust);
  background: var(--ctp-mauve);
  border: 0;
  border-radius: 999px;
  cursor: pointer;
}
.dock-circle.is-stop {
  color: var(--ctp-text);
  background: var(--ctp-red);
}
.dock-circle:disabled {
  color: var(--ctp-overlay0);
  background: var(--ctp-surface1);
  cursor: not-allowed;
}
.input:disabled {
  color: var(--ctp-overlay0);
  cursor: not-allowed;
}
.actions {
  display: grid;
  gap: var(--space-2);
}
.disabled-reason,
.send-error {
  margin: 0 0 var(--space-2);
  font-size: var(--font-small);
}
.disabled-reason {
  color: var(--ctp-subtext0);
}
.send-error {
  color: var(--ctp-red);
}
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
</style>
