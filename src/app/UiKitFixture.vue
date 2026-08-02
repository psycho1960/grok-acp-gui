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
</script>
<template><main class="kit" aria-labelledby="kit-title"><header><p class="eyebrow">Development only</p><h1 id="kit-title">UI component matrix</h1></header><section><h2>Buttons</h2><div class="row"><Button>Default</Button><Button variant="primary">Primary</Button><Button variant="danger">Danger</Button><Button state="loading">Loading</Button><Button disabled>Disabled</Button><Tooltip text="Accessible tooltip"><IconButton label="示例图标按钮">◎</IconButton></Tooltip></div></section><section><h2>Fields</h2><div class="fields"><Input v-model="field" label="Input" placeholder="Type here" /><Input label="Error input" error="Required field" /><Textarea v-model="field" label="Textarea" /><Select v-model="field" label="Select" :options="[{ value: '', label: 'Choose one' }, { value: 'plan', label: 'Plan' }]" /></div></section><section><h2>Status</h2><div class="row"><Badge>Neutral</Badge><Badge tone="info">Info</Badge><Badge tone="success">Success</Badge><Badge tone="warning">Warning</Badge><Badge tone="danger">Error</Badge><StatusIcon status="running" label="运行中" /><StatusIcon status="waiting" label="等待审批" /><StatusIcon status="success" label="已完成" /><StatusIcon status="error" label="失败" /></div></section><section><h2>States</h2><EmptyState title="空状态" detail="这里没有可显示的内容。" /><ErrorState title="错误状态" detail="操作未能完成。" /><Skeleton height="24px" /></section><section><h2>Overlays</h2><div class="row"><Button @click="dialogOpen = true">Open dialog</Button><Button @click="drawerOpen = true">Open drawer</Button></div></section><Dialog v-model="dialogOpen" title="示例对话框" description="Esc 关闭，焦点将被限制在对话框内。"><p>Dialog content</p><template #actions><Button @click="dialogOpen = false">Cancel</Button><Button variant="primary" @click="dialogOpen = false">Confirm</Button></template></Dialog><Drawer v-model="drawerOpen" title="示例抽屉"><p>窄屏 Inspector 降级示例。</p></Drawer></main></template>
<style scoped>.kit { width:min(1000px, calc(100% - 32px)); margin:0 auto; padding:var(--space-8) 0; }.kit h1, .kit h2 { margin:0; }.kit h1 { font-size:20px; }.kit h2 { font-size:16px; }.kit section { display:grid; gap:var(--space-3); margin-top:var(--space-6); }.row { display:flex; flex-wrap:wrap; gap:var(--space-2); align-items:center; }.fields { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); gap:var(--space-3); }.fields :deep(textarea) { min-height:88px; }@media (max-width:600px) { .fields { grid-template-columns:1fr; } }</style>
