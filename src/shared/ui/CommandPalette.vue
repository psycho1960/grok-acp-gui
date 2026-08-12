<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import Dialog from "./Dialog.vue";
import NamedIcon from "./NamedIcon.vue";
import type { IconName } from "./icons";

export type CommandItem = {
  id: string;
  label: string;
  hint?: string;
  group: string;
  icon?: IconName;
  keywords?: string;
  run: () => void;
};

const props = defineProps<{
  modelValue: boolean;
  items: CommandItem[];
}>();

const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();

const query = ref("");
const activeIndex = ref(0);
const inputEl = ref<HTMLInputElement | null>(null);

const filtered = computed(() => {
  const q = query.value.trim().toLocaleLowerCase();
  if (!q) return props.items;
  return props.items.filter((item) => {
    const hay = `${item.label} ${item.hint ?? ""} ${item.group} ${item.keywords ?? ""}`.toLocaleLowerCase();
    return hay.includes(q);
  });
});

const grouped = computed(() => {
  const map = new Map<string, CommandItem[]>();
  for (const item of filtered.value) {
    const list = map.get(item.group) ?? [];
    list.push(item);
    map.set(item.group, list);
  }
  return [...map.entries()];
});

const flat = computed(() => filtered.value);

watch(
  () => props.modelValue,
  async (open) => {
    if (open) {
      query.value = "";
      activeIndex.value = 0;
      await nextTick();
      inputEl.value?.focus();
    }
  },
);

watch(filtered, () => {
  activeIndex.value = 0;
});

function close(): void {
  emit("update:modelValue", false);
}

function run(item: CommandItem): void {
  close();
  item.run();
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    if (!flat.value.length) return;
    activeIndex.value = (activeIndex.value + 1) % flat.value.length;
    return;
  }
  if (event.key === "ArrowUp") {
    event.preventDefault();
    if (!flat.value.length) return;
    activeIndex.value =
      (activeIndex.value - 1 + flat.value.length) % flat.value.length;
    return;
  }
  if (event.key === "Enter") {
    event.preventDefault();
    const item = flat.value[activeIndex.value];
    if (item) run(item);
  }
}

function isActive(item: CommandItem): boolean {
  return flat.value[activeIndex.value]?.id === item.id;
}
</script>

<template>
  <Dialog
    :model-value="modelValue"
    title="命令面板"
    description="搜索任务或跳转页面。Ctrl+K 随时打开。"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <div class="palette" data-testid="command-palette" @keydown="onKeydown">
      <label class="sr-only" for="command-palette-input">搜索命令</label>
      <input
        id="command-palette-input"
        ref="inputEl"
        v-model="query"
        class="palette-input"
        type="search"
        placeholder="搜索任务、页面…"
        data-testid="command-palette-input"
        autocomplete="off"
      />
      <div class="results" role="listbox" aria-label="命令结果">
        <p v-if="!flat.length" class="empty" role="status">无匹配结果</p>
        <section v-for="[group, groupItems] in grouped" :key="group">
          <h3>{{ group }}</h3>
          <button
            v-for="item in groupItems"
            :key="item.id"
            type="button"
            class="result"
            :class="{ active: isActive(item) }"
            role="option"
            :aria-selected="isActive(item)"
            :data-testid="`command-item-${item.id}`"
            @mouseenter="activeIndex = flat.indexOf(item)"
            @click="run(item)"
          >
            <NamedIcon v-if="item.icon" :name="item.icon" :size="16" />
            <span class="label">{{ item.label }}</span>
            <span v-if="item.hint" class="hint">{{ item.hint }}</span>
          </button>
        </section>
      </div>
    </div>
  </Dialog>
</template>

<style scoped>
.palette {
  display: grid;
  gap: var(--space-3);
}
.palette-input {
  width: 100%;
  min-height: var(--button-height);
  padding: 0 var(--space-3);
  color: var(--ctp-text);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
.results {
  max-height: 320px;
  overflow: auto;
  display: grid;
  gap: var(--space-3);
}
.results h3 {
  margin: 0 0 var(--space-1);
  color: var(--ctp-overlay0);
  font-size: var(--text-xs);
  letter-spacing: 0.04em;
}
.result {
  display: grid;
  grid-template-columns: 16px 1fr auto;
  gap: var(--space-2);
  align-items: center;
  width: 100%;
  min-height: 36px;
  padding: var(--space-2);
  color: var(--ctp-text);
  text-align: left;
  cursor: pointer;
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-control);
}
.result:hover,
.result.active {
  background: var(--overlay-active);
  border-color: color-mix(in srgb, var(--ctp-mauve) 35%, var(--ctp-surface1));
}
.label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hint {
  color: var(--ctp-subtext0);
  font-size: var(--font-small);
}
.empty {
  margin: 0;
  color: var(--ctp-subtext0);
}
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  border: 0;
}
</style>
