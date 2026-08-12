<script setup lang="ts">
import { ref } from "vue";
import Badge from "../shared/ui/Badge.vue";
import Button from "../shared/ui/Button.vue";
import Dialog from "../shared/ui/Dialog.vue";
import Drawer from "../shared/ui/Drawer.vue";
import EmptyState from "../shared/ui/EmptyState.vue";
import ErrorState from "../shared/ui/ErrorState.vue";
import IconButton from "../shared/ui/IconButton.vue";
import Input from "../shared/ui/Input.vue";
import Select from "../shared/ui/Select.vue";
import Skeleton from "../shared/ui/Skeleton.vue";
import StatusIcon from "../shared/ui/StatusIcon.vue";
import Textarea from "../shared/ui/Textarea.vue";
import Tooltip from "../shared/ui/Tooltip.vue";
const dialogOpen = ref(false); const drawerOpen = ref(false); const field = ref("");
const stateMatrix = ["default", "hover", "focus", "active", "disabled", "loading", "error"] as const;
const selectOptions = [{ value: "", label: "请选择" }, { value: "plan", label: "计划" }];
</script>
<template><main class="kit" aria-labelledby="kit-title"><header><p class="eyebrow">仅用于开发</p><h1 id="kit-title">UI 组件矩阵</h1></header><section><h2>按钮</h2><div class="row"><Button v-for="state in stateMatrix" :key="state" :state="state">{{ state }}</Button><Tooltip v-for="state in stateMatrix" :key="`tooltip-${state}`" text="无障碍提示"><IconButton :label="`图标按钮 ${state}`" :state="state">◎</IconButton></Tooltip></div></section><section><h2>输入框</h2><div class="fields"><Input v-for="state in stateMatrix" :key="`input-${state}`" v-model="field" :label="`输入框 ${state}`" placeholder="在此输入" :state="state" :error="state === 'error' ? '必填字段' : undefined" /><Textarea v-for="state in stateMatrix" :key="`textarea-${state}`" v-model="field" :label="`文本域 ${state}`" :state="state" :error="state === 'error' ? '必填字段' : undefined" /><Select v-for="state in stateMatrix" :key="`select-${state}`" v-model="field" :label="`下拉框 ${state}`" :state="state" :options="selectOptions" /></div></section><section><h2>状态</h2><div class="row"><Badge>中性</Badge><Badge tone="info">信息</Badge><Badge tone="success">成功</Badge><Badge tone="warning">警告</Badge><Badge tone="danger">错误</Badge><StatusIcon status="running" label="运行中" /><StatusIcon status="waiting" label="等待审批" /><StatusIcon status="success" label="已完成" /><StatusIcon status="error" label="失败" /></div></section><section><h2>状态组件</h2><EmptyState title="空状态" detail="这里没有可显示的内容。" /><ErrorState title="错误状态" detail="操作未能完成。" /><Skeleton height="24px" /></section><section><h2>浮层</h2><div class="row"><Button @click="dialogOpen = true">打开对话框</Button><Button @click="drawerOpen = true">打开抽屉</Button></div></section><Dialog v-model="dialogOpen" title="示例对话框" description="Esc 关闭，焦点将被限制在对话框内。"><p>对话框内容</p><template #actions><Button @click="dialogOpen = false">取消</Button><Button variant="primary" @click="dialogOpen = false">确认</Button></template></Dialog><Drawer v-model="drawerOpen" title="示例抽屉"><p>窄屏检查器降级示例。</p></Drawer></main></template>
<style scoped>.kit { width:min(1000px, calc(100% - 32px)); margin:0 auto; padding:var(--space-8) 0; }.kit h1, .kit h2 { margin:0; }.kit h1 { font-size:var(--heading-page); line-height:var(--leading-tight); }.kit h2 { font-size:var(--heading-panel); line-height:var(--leading-tight); }.kit section { display:grid; gap:var(--space-3); margin-top:var(--space-6); }.row { display:flex; flex-wrap:wrap; gap:var(--space-2); align-items:center; }.fields { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); gap:var(--space-3); }.fields :deep(textarea) { min-height:88px; }@media (max-width:600px) { .fields { grid-template-columns:1fr; } }</style>
