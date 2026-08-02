<script setup lang="ts">
import { computed, h, ref } from "vue";
import AppShell from "./AppShell.vue";
import Badge from "../shared/ui/Badge.vue";
import Button from "../shared/ui/Button.vue";
import EmptyState from "../shared/ui/EmptyState.vue";
import StatusIcon from "../shared/ui/StatusIcon.vue";

const inspectorOpen = ref(true);
const left = computed(() => h("nav", { class: "nav-list", "aria-label": "任务分组" }, [h("h2", "Tasks"), h("button", { class: "nav-item", type: "button" }, "Running"), h("button", { class: "nav-item", type: "button" }, "Needs attention"), h("button", { class: "nav-item", type: "button" }, "Review"), h("button", { class: "nav-item", type: "button" }, "Completed")]));
const main = computed(() => h("div", { class: "main-empty" }, [h(EmptyState, { title: "没有打开的任务", detail: "选择一个任务后，对话、计划和工具时间线将显示在这里。" }, { default: () => h(Button, { class: "new-task", variant: "primary" }, { default: () => "新建任务" }) })]));
const inspector = computed(() => h("section", { class: "inspector-content" }, [h("h2", "Inspector"), h(StatusIcon, { status: "waiting", label: "等待选择任务" }), h("p", "变更、Diff、Artifacts 和 Worktree 信息将在选择任务后显示。"), h(Badge, { tone: "neutral" }, { default: () => "空状态" })]));
const statusBar = computed(() => h("span", "未选择项目 · 桌面壳已就绪"));
</script>
<template><AppShell :left="left" :main="main" :inspector="inspector" :inspector-open="inspectorOpen" :status-bar="statusBar" @update:inspector-open="inspectorOpen = $event" /></template>
<style scoped>.nav-list { display:grid; gap:var(--space-1); }.nav-list h2, .inspector-content h2 { margin:0 0 var(--space-2); color:var(--ctp-text); font-size:16px; }.nav-item { min-height:32px; padding:0 var(--space-2); color:var(--ctp-subtext0); text-align:left; background:transparent; border:1px solid transparent; border-radius:var(--radius-control); cursor:pointer; }.nav-item:hover { color:var(--ctp-text); background:var(--ctp-surface0); }.main-empty { display:grid; min-height:100%; place-items:center; }.new-task { margin-top:var(--space-4); }.inspector-content { display:grid; gap:var(--space-3); }.inspector-content p { margin:0; color:var(--ctp-subtext0); }</style>
