<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { bootstrap, type BootstrapStatus } from "./bridge/desktop-bridge";
import ErrorState from "./shared/ui/ErrorState.vue";
import ShellView from "./app/ShellView.vue";
import UiKitFixture from "./app/UiKitFixture.vue";
import TaskCenterFixture from "./features/task-center/TaskCenterFixture.vue";
import { parseTaskCenterHash } from "./features/task-center/hash-route";
import ConversationFixture from "./features/conversation/ConversationFixture.vue";
import { parseConversationHash } from "./features/conversation/hash-route";

const isLoading = ref(true);
const startupError = ref<string | null>(null);
const bootstrapStatus = ref<BootstrapStatus | null>(null);
/** True only when bootstrap threw (no Tauri host) — not when DB is unavailable. */
const bootstrapThrew = ref(false);
const routeHash = ref(typeof window !== "undefined" ? window.location.hash : "");

function syncHash(): void {
  routeHash.value = window.location.hash;
}

const developmentRoute = computed(() => routeHash.value);

const showUiKit = computed(() => developmentRoute.value === "#ui-kit");
const showShellPreview = computed(() => developmentRoute.value === "#shell");
const conversationRoute = computed(() => parseConversationHash(routeHash.value));

// UI-ERROR-001: when the database is unavailable or corrupt the backend
// returns `ready:false` with `dbError`. Prefer this over fixture.
const dbUnavailable = computed(
  () =>
    bootstrapStatus.value != null &&
    (bootstrapStatus.value.ready === false || !!bootstrapStatus.value.dbError),
);
const dbErrorDetail = computed(
  () =>
    bootstrapStatus.value?.dbError ??
    "Application data is unavailable. Restart the application; if the problem persists, contact support.",
);

/**
 * Fixture only when host is missing (bootstrap threw). Never mask UI-ERROR-001
 * for ready===false / dbError responses.
 */
const showTaskCenterFixture = computed(() => {
  const route = parseTaskCenterHash(routeHash.value);
  if (!route.active) return false;
  if (dbUnavailable.value) return false;
  if (bootstrapStatus.value?.ready) return false;
  return bootstrapThrew.value || bootstrapStatus.value == null;
});

const showConversationFixture = computed(() => {
  if (!conversationRoute.value.active) return false;
  if (dbUnavailable.value) return false;
  if (bootstrapStatus.value?.ready) return false;
  return bootstrapThrew.value || bootstrapStatus.value == null;
});

onMounted(async () => {
  window.addEventListener("hashchange", syncHash);

  if (showUiKit.value || showShellPreview.value) {
    isLoading.value = false;
    return;
  }

  // Conversation hash: try bootstrap; fixture only if invoke throws (no host).
  if (conversationRoute.value.active) {
    try {
      bootstrapStatus.value = await bootstrap();
      bootstrapThrew.value = false;
    } catch {
      bootstrapStatus.value = null;
      bootstrapThrew.value = true;
    } finally {
      isLoading.value = false;
    }
    return;
  }

  // Task-center hash: try bootstrap; fixture only if invoke throws (no host).
  if (parseTaskCenterHash(routeHash.value).active) {
    try {
      bootstrapStatus.value = await bootstrap();
      bootstrapThrew.value = false;
    } catch {
      bootstrapStatus.value = null;
      bootstrapThrew.value = true;
    } finally {
      isLoading.value = false;
    }
    return;
  }

  try {
    bootstrapStatus.value = await bootstrap();
    bootstrapThrew.value = false;
  } catch (error) {
    startupError.value = error instanceof Error ? error.message : String(error);
    bootstrapThrew.value = true;
  } finally {
    isLoading.value = false;
  }
});

onUnmounted(() => {
  window.removeEventListener("hashchange", syncHash);
});
</script>

<template>
  <UiKitFixture v-if="showUiKit" />
  <ShellView v-else-if="showShellPreview" />
  <main v-else-if="isLoading" class="startup">
    <p role="status">正在启动 Grok ACP GUI…</p>
  </main>
  <main v-else-if="startupError" class="startup">
    <ErrorState title="启动失败" :detail="startupError" />
  </main>
  <main v-else-if="dbUnavailable" class="startup">
    <ErrorState title="数据库不可用" :detail="dbErrorDetail" data-err="UI-ERROR-001" />
  </main>
  <ConversationFixture v-else-if="showConversationFixture" />
  <TaskCenterFixture v-else-if="showTaskCenterFixture" />
  <ShellView v-else :data-version="bootstrapStatus?.version" />
</template>

<style scoped>
.startup {
  display: grid;
  min-height: 100vh;
  padding: var(--space-6);
  place-items: center;
  background: var(--ctp-base);
}
.startup > p {
  color: var(--ctp-subtext0);
}
</style>
