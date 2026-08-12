# Grok ACP GUI 视觉与用户体验优化方案

> **依据**: `docs/UI-UX-AUDIT-REPORT.md`（v0.1.16 审计）  
> **对齐**: `docs/02-UI-UX-DESIGN.md`（任务控制台、可观察、渐进披露、状态诚实、无色盲陷阱）  
> **产品定位**: 开发者桌面生产力工具  
> **落盘日期**: 2026-08-12  
> **实施任务**: Phase 0 → `feat/GAG-017-ui-ux-foundation`（及后续分票）

---

## 0. 方案总则

### 0.1 优化北极星

| 原则 | 在本项目中的含义 | 对应审计短板 |
|------|------------------|--------------|
| **任务控制台质感** | 状态、位置、风险一眼可读 | 信息密度失衡、状态栏闲置 |
| **设计系统闭环** | 字号/阴影/断点/图标全部 Token 化 | 排版、阴影、图标、断点 P0 |
| **反馈闭环** | 任何操作都有可见结果 | 无 Toast、长操作无进度 |
| **渐进披露** | 默认干净，高级能力可发现 | TaskCenter 头部过载、快捷键不可见 |
| **安全默认可懂** | 危险操作路径短但不可误触 | Worktree 清理确认过长 |

### 0.2 与现有规范的冲突（需决策，不可静默改）

| 审计建议 | 现行设计规范 | 决议 |
|----------|--------------|------|
| P1 增加 Catppuccin Latte 亮色主题 | UI-SETTINGS：v1 **固定 Mocha** | **v0.2 不做主题切换**；Token 结构可预留扩展，亮色需独立 ADR |
| 最小高度降至 500px | 规范最小 1024×680 | **暂不改窗口最小高度**；侧栏独立滚动可后续做 |
| 状态栏塞满 Git/模型信息 | 规范已规定 cwd · session · model · reasoning · diagnostics | **按规范补齐**（Phase 1） |

### 0.3 版本切片

| 版本 | 主题 | 范围 | 预估 |
|------|------|------|------|
| **v0.2.0 质感基线** | Design System 硬化 + 全局反馈 | P0 全部 + 无争议小项 | ~16–20h |
| **v0.3.0 信息架构** | 密度、导航、引导、动效 | 剩余 P1 | ~24–28h |
| **v0.4.0 精炼** | 文案、搜索、对比度 CI、亮色（若 ADR） | P2 | 按项 |
| **Backlog** | 微交互与边缘场景 | P3 | 按需 |

---

## 1. 工作流 A：设计系统硬化

### A1. 排版层级 Token（P0-1 / P0-5）

```css
--text-xs: 11px;
--text-sm: 12px;
--text-base: 14px;
--text-lg: 16px;
--text-xl: 18px;
--text-2xl: 20px;
--text-3xl: 24px;
--text-4xl: 28px;

--heading-page: var(--text-3xl);      /* 24px — 页面主标题 */
--heading-section: var(--text-xl);    /* 18px — 模块标题 */
--heading-panel: var(--text-lg);      /* 16px — Drawer/Inspector */
--heading-card: var(--text-base);     /* 14px — 列表项标题 */
--heading-dialog: var(--text-2xl);    /* 20px — Dialog 标题 */

--leading-tight: 1.25;
--leading-normal: 1.43;
--leading-relaxed: 1.6;

--font-weight-regular: 400;
--font-weight-medium: 500;
--font-weight-semibold: 600;
--font-weight-bold: 700;
```

| 位置 | 目标 |
|------|------|
| Onboarding 欢迎 h1 | `.heading-display` → `--text-4xl`（展示用，非 page） |
| TaskCenter / Review / Recovery h1 | `--heading-page` |
| Dialog h2 | `--heading-dialog` |
| Drawer h2 / Empty / Error | `--heading-panel` |
| TaskCard title | `--heading-card` + `--leading-tight` |
| `font-weight: 650` | → `600` |

### A2. 阴影与高度（P0-3）

```css
--shadow-sm: 0 1px 2px rgb(0 0 0 / 0.30);
--shadow-md: 0 4px 12px rgb(0 0 0 / 0.40);
--shadow-lg: 0 12px 40px rgb(0 0 0 / 0.50);
--shadow-xl: 0 24px 64px rgb(0 0 0 / 0.55);
--elevation-menu: var(--shadow-md);
--elevation-drawer: var(--shadow-lg);
--elevation-modal: var(--shadow-xl);
```

### A3. 断点（P0-6 / P1-11）

```ts
export const BREAKPOINTS = { xl: 1200, lg: 1080, md: 900, sm: 720, xs: 640 } as const
```

- `useResponsive()` 导出 `isDrawerMode` / `isCompactNav` / `isFixedLeft` 等
- AppShell 与业务组件逐步收敛到同一常量

### A4. SVG 图标系统（P0-2）

- 轻量内联 SVG（`Icon.vue` + 具名图标），避免 Unicode 跨平台不一致
- 默认 16×16；导航/顶栏 20×20；`currentColor`；热区 ≥32×32
- StatusIcon 保持 **颜色 + 图标 + 文字** 三通道

### A5. 间距 / 圆角 / 遮罩补全

```css
--space-5: 20px;
--space-10: 40px;
--space-12: 48px;
--radius-panel: 8px;
--backdrop-alpha: 72%;
--overlay-hover / --overlay-active / --overlay-info / --overlay-danger
```

Badge：warning/danger 背景上使用足够对比的文字色。

### A6. 等宽字体栈

```css
--font-mono: "Cascadia Code", "Cascadia Mono", "Fira Code", Consolas, "Courier New", monospace;
```

---

## 2. 工作流 B：全局反馈

### B1. Toast（P0-4）

| 项 | 规格 |
|----|------|
| 位置 | 主内容右上 |
| 栈 | 最多 3 条 |
| 时长 | success/info 3s；warning 5s；error 不自动关或 8s |
| API | `toast.success/error/warning/info` |
| 动效 | 入场/出场；尊重 `prefers-reduced-motion` |

最小接入：打开项目、建任务失败、Checkpoint、取消任务、复制成功、会话失效。

### B2–B4（Phase 1+）

长操作步进、Dialog/Drawer 动效、断连体验统一。

---

## 3. 工作流 C：信息架构（Phase 1）

- TaskCenter 头部：2 行主路径 + 可折叠筛选；去掉与左栏重复的 chip 行
- 顶栏面包屑
- 状态栏按设计规范填满
- 左栏分组 + 图标 + 激活竖条
- Review 窄屏 Checkpoint 折叠

---

## 4. 工作流 D–F（Phase 1–2）

可发现性（Slash `?`、快捷键面板）、首次引导、a11y 虚拟列表、文案统一、Worktree 清理确认简化等——详见审计报告 §二–§六。

---

## 5. 实施路线图

### Phase 0 — v0.2.0 质感基线（GAG-017）✅

| 序号 | 交付物 | 来源 | 工时 |
|------|--------|------|------|
| 0.1 | 排版 + 语义 heading Token + 标题迁移 | P0-1/5 | 3h |
| 0.2 | 阴影 / radius-panel / backdrop / overlay Token | P0-3, P1-10 | 2h |
| 0.3 | 断点常量 + `useResponsive` | P0-6 | 2h |
| 0.4 | Toast 组件 + 宿主挂载 + 核心事件 | P0-4 | 4h |
| 0.5 | Icon 系统 + StatusIcon / 关键按钮 | P0-2 | 6h |
| 0.6 | Review Input、font-weight、banner 合并、Badge 对比度 | 杂项 | 1h |

**出口标准**: 页面标题一致；无 Unicode 关键状态图标；Token 缺口补齐；Toast 可用。

### Phase 1 — v0.3.0 控制台成型（GAG-018）✅

| 序号 | 交付物 | 状态 |
|------|--------|------|
| 1.1 | TaskCenter 头部：搜索常驻 + 筛选折叠 + 项目菜单 | 完成 |
| 1.2 | 左栏分组/图标/激活竖条 + 顶栏面包屑 | 完成 |
| 1.3 | 状态栏三区（路径 / session / model） | 完成 |
| 1.4 | Dialog/Drawer 入场动效 | 完成 |
| 1.5 | Checkpoint 进度文案 | 完成（集成长操作步进可后续加深） |
| 1.6 | Composer 可发现性 + placeholder + 快捷键帮助 | 完成 |
| 1.7 | 首次引导浮层 | 完成 |
| 1.8 | VirtualList a11y rowcount/posinset | 完成 |
| 1.9 | Review 返回 + 窄屏 Checkpoint 折叠 | 完成 |

### Phase 2 — v0.4.0 精炼（GAG-019）✅

| 序号 | 交付物 | 状态 |
|------|--------|------|
| 2.1 | 错误消息映射 + ErrorState 复制详情 | 完成 |
| 2.2 | Ctrl+K 命令面板 | 完成 |
| 2.3 | 模式帮助 Tooltip + Permission 过渡 | 完成 |
| 2.4 | Recovery/Worktree 文案精炼 | 完成 |
| 2.5 | overlay Token 收敛 + contrast CI | 完成 |
| 2.6 | 亮色主题 ADR | **不做**（仍固定 Mocha） |

### Phase 3 — 微交互与边缘体验（GAG-020）✅

| 序号 | 交付物 | 状态 |
|------|--------|------|
| 3.1 | 回到底部过渡（常驻 DOM + visible 类） | 完成 |
| 3.2 | Button/IconButton 按下缩放 + SVG spinner | 完成 |
| 3.3 | Tooltip 300ms 延迟 | 完成 |
| 3.4 | Skip-to-content + Resizer 键盘提示 | 完成 |
| 3.5 | Drawer 右滑关闭 | 完成 |
| 3.6 | 打印样式 | 完成 |
| 3.7 | Worktree 强制清理 UI：`DELETE`（后端仍收路径） | 完成 |
| 3.8 | 1.5dppx 微调 | 完成 |

**仍不在范围**：亮色主题、Ctrl 以外的完整快捷键重映射、真实触控手势库。

---

## 6. 验收矩阵

| ID | 场景 | 通过条件 |
|----|------|----------|
| V-01 | 多页面主标题 | 使用 `--heading-page`（展示页除外） |
| V-02 | 系统字体变化 | 状态图标不裂、不方框 |
| V-03 | Toast API | success/error 可弹出与关闭 |
| V-04 | Token | `radius-panel`、阴影、断点存在 |
| V-05 | Badge warning | 文字对比度足够 |
| V-06 | reduced-motion | 动画可降级 |

自动化：`npm run typecheck`、`npm run lint`、相关 `test:node` / `test:ui`。

---

## 7. 风险与约束

| 风险 | 缓解 |
|------|------|
| 图标增加包体 | 内联 SVG + 仅使用的图标 |
| Toast 与权限卡抢注意力 | 权限仍以内联为准；Toast 只报结果级事件 |
| 扩大范围破坏任务边界 | Phase 分分支；禁止顺手改业务逻辑 |
| 与 UI-SETTINGS 主题冲突 | 不做假主题开关 |

---

## 8. 优先级一句话

**先锁 Token 与 Toast（质感+反馈），再收 TaskCenter 密度与状态栏（控制台感），然后动效与引导（精致度），最后文案/搜索/亮色（广度）。**
