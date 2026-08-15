<script setup lang="ts">
import SafeMarkdown from "./SafeMarkdown.vue";
import ToolCard from "./ToolCard.vue";
import ArtifactSlot from "./slots/ArtifactSlot.vue";
import PermissionSlot from "./slots/PermissionSlot.vue";
import PlanSlot from "./slots/PlanSlot.vue";
import { ref } from "vue";
import { formatDuration } from "./tool-normalize";
import type { TimelineItem } from "./types";

const props = defineProps<{
  item: TimelineItem;
  focused?: boolean;
  showTime?: boolean;
  thinkingDone?: boolean;
  previewUrls?: Record<string, string>;
  previewMissing?: Record<string, boolean>;
}>();

const brokenThumbs = ref<Record<string, boolean>>({});

function thumbUrl(artifactId: string): string | undefined {
  if (brokenThumbs.value[artifactId]) return undefined;
  return props.previewUrls?.[artifactId];
}

function thumbMissing(artifactId: string): boolean {
  return Boolean(brokenThumbs.value[artifactId] || props.previewMissing?.[artifactId]);
}

function onThumbError(artifactId: string): void {
  brokenThumbs.value = { ...brokenThumbs.value, [artifactId]: true };
}

const emit = defineEmits<{
  toggleTool: [id: string];
  toggleThinking: [id: string];
  resolvePermission: [itemId: string, optionId: string];
  resolvePlan: [itemId: string, optionId: string];
  openArtifact: [artifactId: string];
}>();

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

function formatRelativeTime(iso: string): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const minutes = Math.floor(Math.max(0, Date.now() - then) / 60_000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
}
</script>

<template>
  <div
    class="timeline-item"
    :class="[`kind-${item.kind}`, { focused }]"
    :data-testid="`timeline-item-${item.kind}`"
    :data-align="item.kind === 'user' ? 'end' : 'start'"
    :data-item-id="item.id"
    :data-seq="item.seq"
    :data-event-key="item.eventKey"
    role="listitem"
  >
    <time
      v-if="showTime"
      class="relative-time"
      data-testid="relative-time"
      :datetime="item.timestamp"
      :title="formatTime(item.timestamp)"
    >
      {{ formatRelativeTime(item.timestamp) }}
    </time>
    <template v-if="item.kind === 'user'">
      <div class="lane user-lane">
        <div
          class="bubble user"
          :class="{ pending: item.pending }"
          :data-pending="item.pending ? 'true' : undefined"
          :style="item.pending ? { opacity: 0.55 } : undefined"
          data-testid="user-message"
        >
          <p>{{ item.text }}</p>
          <ul v-if="item.attachments?.length" class="message-attachments" aria-label="消息附件">
            <li v-for="attachment in item.attachments" :key="attachment.artifactId">
              <button
                v-if="attachment.mimeType.startsWith('image/')"
                type="button"
                class="user-thumb-btn"
                :aria-label="attachment.displayName"
                @click="emit('openArtifact', attachment.artifactId)"
              >
                <img
                  v-if="thumbUrl(attachment.artifactId)"
                  data-testid="user-thumb"
                  width="72"
                  height="72"
                  :src="thumbUrl(attachment.artifactId)"
                  :alt="attachment.displayName"
                  @error="onThumbError(attachment.artifactId)"
                />
                <span
                  v-else-if="thumbMissing(attachment.artifactId)"
                  class="user-thumb-missing"
                  data-testid="user-thumb-missing"
                >
                  <span class="missing-name">{{ attachment.displayName }}</span>
                  <span class="missing-hint">找不到图片缓存</span>
                </span>
                <span
                  v-else
                  class="user-thumb-missing"
                  data-testid="user-thumb-loading"
                  :aria-label="attachment.displayName"
                />
              </button>
              <template v-else>
                <span>{{ attachment.displayName }}</span>
                <span class="attachment-size">{{ Math.ceil(attachment.bytes / 1024) }} KiB</span>
              </template>
            </li>
          </ul>
          <p v-if="item.pending" class="status-line pending-whisper">发送中</p>
          <p v-if="item.failed" class="status-line error" role="alert">
            {{ item.errorMessage ?? "发送失败" }}
          </p>
        </div>
      </div>
    </template>

    <template v-else-if="item.kind === 'assistant'">
      <div class="prose assistant" data-chrome="prose" data-testid="assistant-message">
        <SafeMarkdown :source="item.text" :streaming="item.streaming" />
      </div>
    </template>

    <template v-else-if="item.kind === 'thinking'">
      <button
        type="button"
        class="thinking"
        data-testid="thinking-toggle"
        :aria-expanded="item.expanded"
        @click="emit('toggleThinking', item.id)"
      >
        {{ thinkingDone ? "已思考" : "思考中" }}
        <span v-if="item.durationMs != null" class="dur">{{ formatDuration(item.durationMs) }}</span>
      </button>
      <p v-if="item.expanded && item.summary" class="thinking-body" data-testid="thinking-body">{{ item.summary }}</p>
    </template>

    <template v-else-if="item.kind === 'tool'">
      <ToolCard
        :tool="item.tool"
        :expanded="item.expanded"
        @toggle="emit('toggleTool', item.id)"
      />
    </template>

    <template v-else-if="item.kind === 'permission'">
      <PermissionSlot :slot-data="item.slot" @resolve="emit('resolvePermission', item.id, $event)" />
    </template>

    <template v-else-if="item.kind === 'plan'">
      <PlanSlot :slot-data="item.slot" @resolve="emit('resolvePlan', item.id, $event)" />
    </template>

    <template v-else-if="item.kind === 'artifact'">
      <ArtifactSlot :slot-data="item.slot" @open="emit('openArtifact', $event)" />
    </template>

    <template v-else-if="item.kind === 'error'">
      <div class="bubble error" role="alert" data-testid="error-item">
        <code v-if="item.code" class="error-code">{{ item.code }}</code>
        <p>{{ item.message }}</p>
      </div>
    </template>

    <template v-else-if="item.kind === 'activity' && item.activityKind === 'changes'">
      <p class="change-whisper" data-testid="change-whisper">{{ item.detail }}</p>
    </template>

    <template v-else-if="item.kind === 'activity' || item.kind === 'system'">
      <p class="system-line" data-testid="system-item">
        <template v-if="item.kind === 'activity'">{{ item.detail }}</template>
        <template v-else>{{ item.message }}</template>
      </p>
    </template>

    <template v-else-if="item.kind === 'unknown'">
      <div class="bubble unknown" data-testid="unknown-item">
        <p class="type">{{ item.eventType }}</p>
        <p>{{ item.safeSummary }}</p>
        <p class="hint">原始 JSON 默认不展示</p>
      </div>
    </template>
  </div>
</template>

<style scoped>
.timeline-item {
  padding: var(--space-2) var(--space-3);
  border-left: 2px solid transparent;
}
.relative-time {
  display: block;
  margin-bottom: var(--space-1);
  color: var(--ctp-overlay0);
  font-size: var(--font-small);
}
.timeline-item[data-align="end"] .user-lane {
  display: flex;
  justify-content: flex-end;
}
.user-lane .bubble {
  max-width: min(72ch, 86%);
}
.prose.assistant {
  max-width: 72ch;
  padding: 0;
  background: transparent;
  border: 0;
}
.timeline-item.focused {
  border-left-color: var(--ctp-mauve);
  background: var(--ctp-surface0);
}
.meta-row {
  display: flex;
  gap: var(--space-2);
  align-items: center;
  margin-bottom: var(--space-1);
  color: var(--ctp-overlay1);
  font-size: var(--font-small);
}
.seq {
  font-family: var(--font-mono);
}
.bubble {
  padding: var(--space-3);
  border-radius: var(--radius-card);
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface0);
}
.bubble.user {
  background: var(--ctp-surface0);
}
.bubble.user.pending {
  opacity: 0.55;
}
.pending-whisper {
  text-align: right;
}
.bubble.error {
  border-color: var(--ctp-red);
  color: var(--ctp-red);
}
.error-code {
  display: block;
  margin-bottom: var(--space-1);
  font-family: var(--font-mono);
}
.bubble.unknown {
  border-style: dashed;
}
.bubble p,
.prose p {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.status-line {
  margin-top: var(--space-1);
  font-size: var(--font-small);
  color: var(--ctp-subtext0);
}
.message-attachments {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding: 0;
  margin: var(--space-2) 0 0;
  list-style: none;
}
.message-attachments li {
  display: inline-flex;
  max-width: 100%;
  gap: var(--space-1);
  align-items: center;
}
.user-thumb-btn {
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
}
.user-thumb-btn img,
.user-thumb-missing {
  display: grid;
  width: 72px;
  height: 72px;
  place-content: center;
  object-fit: cover;
  background: var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.user-thumb-missing {
  gap: 2px;
  padding: 4px;
  color: var(--ctp-subtext0);
  text-align: center;
}
.missing-name,
.missing-hint {
  display: block;
  overflow: hidden;
  font-size: 10px;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.attachment-size { color: var(--ctp-subtext0); font-size: var(--font-small); }
.status-line.error {
  color: var(--ctp-red);
}
.thinking {
  min-height: 32px;
  padding: 0;
  color: var(--ctp-overlay0);
  background: transparent;
  border: 0;
  cursor: pointer;
}
.thinking-body {
  margin: var(--space-1) 0 0;
  color: var(--ctp-overlay1);
  font-size: var(--font-small);
}
.system-line,
.change-whisper {
  margin: 0;
  color: var(--ctp-overlay0);
  font-size: var(--font-small);
}
.unknown .type {
  font-family: var(--font-mono);
  color: var(--ctp-peach);
}
.unknown .hint {
  margin-top: var(--space-1);
  color: var(--ctp-overlay0);
  font-size: var(--font-small);
}
</style>
