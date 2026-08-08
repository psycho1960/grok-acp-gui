<script setup lang="ts">
import Badge from "../../shared/ui/Badge.vue";
import SafeMarkdown from "./SafeMarkdown.vue";
import ToolCard from "./ToolCard.vue";
import ArtifactSlot from "./slots/ArtifactSlot.vue";
import PermissionSlot from "./slots/PermissionSlot.vue";
import PlanSlot from "./slots/PlanSlot.vue";
import type { TimelineItem } from "./types";

defineProps<{
  item: TimelineItem;
  focused?: boolean;
}>();

const emit = defineEmits<{
  toggleTool: [id: string];
  toggleThinking: [id: string];
  resolvePermission: [itemId: string, optionId: string];
  resolvePlan: [itemId: string, optionId: string];
  openArtifact: [artifactId: string];
}>();

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return "";
  }
}
</script>

<template>
  <div
    class="timeline-item"
    :class="[`kind-${item.kind}`, { focused }]"
    :data-testid="`timeline-item-${item.kind}`"
    :data-item-id="item.id"
    :data-seq="item.seq"
    :data-event-key="item.eventKey"
    role="listitem"
  >
    <div class="meta-row">
      <Badge
        :tone="
          item.kind === 'user'
            ? 'info'
            : item.kind === 'error'
              ? 'danger'
              : item.kind === 'assistant'
                ? 'neutral'
                : 'neutral'
        "
      >
        {{ item.kind }}
      </Badge>
      <time :datetime="item.timestamp">{{ formatTime(item.timestamp) }}</time>
      <span class="seq">#{{ item.seq }}</span>
    </div>

    <template v-if="item.kind === 'user'">
      <div class="bubble user" data-testid="user-message">
        <p>{{ item.text }}</p>
        <ul v-if="item.attachments?.length" class="message-attachments" aria-label="消息附件">
          <li v-for="attachment in item.attachments" :key="attachment.artifactId">
            <span aria-hidden="true">▧</span>
            <span>{{ attachment.displayName }}</span>
            <span class="attachment-size">{{ Math.ceil(attachment.bytes / 1024) }} KiB</span>
          </li>
        </ul>
        <p v-if="item.pending" class="status-line">发送中…</p>
        <p v-if="item.failed" class="status-line error" role="alert">
          {{ item.errorMessage ?? "发送失败" }}
        </p>
      </div>
    </template>

    <template v-else-if="item.kind === 'assistant'">
      <div class="bubble assistant" data-testid="assistant-message">
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
        Thinking…
        <span v-if="item.durationMs != null" class="dur">{{ item.durationMs }}ms</span>
      </button>
      <p v-if="item.expanded" class="thinking-body">{{ item.summary }}</p>
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

    <template v-else-if="item.kind === 'activity' || item.kind === 'system'">
      <p class="system-line" data-testid="system-item">
        <template v-if="item.kind === 'activity'">{{ item.activityKind }}: {{ item.detail }}</template>
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
.bubble p {
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
  padding: var(--space-1) var(--space-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  background: var(--ctp-mantle);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.attachment-size { color: var(--ctp-subtext0); font-size: var(--font-small); }
.status-line.error {
  color: var(--ctp-red);
}
.thinking {
  min-height: 32px;
  padding: 0 var(--space-2);
  color: var(--ctp-subtext0);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
  cursor: pointer;
}
.thinking-body {
  margin: var(--space-1) 0 0;
  color: var(--ctp-overlay1);
  font-size: var(--font-small);
}
.system-line {
  margin: 0;
  color: var(--ctp-overlay1);
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
