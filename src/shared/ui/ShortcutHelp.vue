<script setup lang="ts">
import Dialog from "./Dialog.vue";

defineProps<{ modelValue: boolean }>();
defineEmits<{ "update:modelValue": [value: boolean] }>();

const groups = [
  {
    title: "全局",
    rows: [
      { keys: "Ctrl+K", desc: "命令面板（搜索任务 / 跳转页面）" },
      { keys: "?", desc: "打开快捷键帮助" },
      { keys: "Esc", desc: "关闭对话框 / 抽屉" },
    ],
  },
  {
    title: "对话",
    rows: [
      { keys: "Enter", desc: "发送消息" },
      { keys: "Shift+Enter", desc: "换行" },
      { keys: "Esc", desc: "停止当前 Turn" },
      { keys: "/", desc: "呼出快捷指令" },
    ],
  },
  {
    title: "任务中心",
    rows: [
      { keys: "筛选", desc: "展开状态 / 项目 / 时间条件" },
      { keys: "左侧导航", desc: "按分组浏览任务" },
    ],
  },
] as const;
</script>

<template>
  <Dialog
    :model-value="modelValue"
    title="键盘快捷键"
    description="常用操作速查（与实际行为一致）。"
    @update:model-value="$emit('update:modelValue', $event)"
  >
    <div class="shortcut-help" data-testid="shortcut-help">
      <section v-for="group in groups" :key="group.title">
        <h3>{{ group.title }}</h3>
        <dl>
          <div v-for="row in group.rows" :key="row.keys" class="row">
            <dt><kbd>{{ row.keys }}</kbd></dt>
            <dd>{{ row.desc }}</dd>
          </div>
        </dl>
      </section>
    </div>
  </Dialog>
</template>

<style scoped>
.shortcut-help {
  display: grid;
  gap: var(--space-4);
}
.shortcut-help h3 {
  margin: 0 0 var(--space-2);
  font-size: var(--text-sm);
  color: var(--ctp-subtext0);
}
.shortcut-help dl {
  margin: 0;
  display: grid;
  gap: var(--space-2);
}
.row {
  display: grid;
  grid-template-columns: minmax(120px, auto) 1fr;
  gap: var(--space-3);
  align-items: center;
}
.row dt {
  margin: 0;
}
.row dd {
  margin: 0;
  color: var(--ctp-text);
}
kbd {
  display: inline-block;
  padding: 2px 8px;
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  color: var(--ctp-text);
  background: var(--ctp-surface0);
  border: 1px solid var(--ctp-surface1);
  border-radius: var(--radius-control);
}
</style>
