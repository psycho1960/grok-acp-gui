<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { bootstrap, type BootstrapStatus } from "./bridge/desktop-bridge";
import ErrorState from "./shared/ui/ErrorState.vue";
import ShellView from "./app/ShellView.vue";
import UiKitFixture from "./app/UiKitFixture.vue";
import TaskCenterFixture from "./features/task-center/TaskCenterFixture.vue";
import { parseTaskCenterHash } from "./features/task-center/hash-route";

const isLoading = ref(true);
const startupError = ref<string | null>(null);
const bootstrapStatus = ref<BootstrapStatus | null>(null);
const routeHash = ref(typeof window !== "undefined" ? window.location.hash : "");

function syncHash(): void {
  routeHash.value = window.location.hash;
}

const developmentRoute = computed(() =>
  import.meta.env.DEV ? routeHash.value : routeHash.value,
);

const showUiKit = computed(() => developmentRoute.value === "#ui-kit");
const showShellPreview = computed(() => developmentRoute.value === "#shell");
const showTaskCenterFixture = computed(() => {
  // Fixture path: always available for deep-link E2E and local UI without Tauri.
  // Production Tauri path uses ShellView + real bridge after bootstrap.
  const route = parseTaskCenterHash(routeHash.value);
  if (!route.active) return false;
  // Prefer fixture when not bootstrapped (DEV / no host). After bootstrap, ShellView hosts Task Center.
  if (bootstrapStatus.value?.ready) return false;
  return true;
});

// UI-ERROR-001: when the database is unavailable or corrupt the backend
// returns `ready:false` with `dbError`. The Renderer must NOT render
// ShellView in that case — otherwise persistence failures are hidden.
const dbUnavailable = computed(
  () => bootstrapStatus.value?.ready === false || !!bootstrapStatus.value?.dbError,
);
const dbErrorDetail = computed(
  () =>
    bootstrapStatus.value?.dbError ??
    "Application data is unavailable. Restart the application; if the problem persists, contact support.",
);

onMounted(async () => {
  window.addEventListener("hashchange", syncHash);

  if (showUiKit.value || showShellPreview.value) {
    isLoading.value = false;
    return;
  }

  // Task-center hash: try bootstrap; on host failure fall back to fixture.
  if (parseTaskCenterHash(routeHash.value).active) {
    try {
      bootstrapStatus.value = await bootstrap();
    } catch {
      // Browser / Playwright without Tauri — use TaskCenterFixture.
      bootstrapStatus.value = null;
    } finally {
      isLoading.value = false;
    }
    return;
  }

  try {
    bootstrapStatus.value = await bootstrap();
  } catch (error) {
    startupError.value = error instanceof Error ? error.message : String(error);
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
  <TaskCenterFixture v-else-if="showTaskCenterFixture && !isLoading" />
  <main v-else-if="isLoading" class="startup">
    <p role="status">正在启动 Grok ACP GUI…</p>
  </main>
  <main v-else-if="startupError" class="startup">
    <ErrorState title="启动失败" :detail="startupError" />
  </main>
  <main v-else-if="dbUnavailable" class="startup">
    <ErrorState title="数据库不可用" :detail="dbErrorDetail" data-err="UI-ERROR-001" />
  </main>
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
