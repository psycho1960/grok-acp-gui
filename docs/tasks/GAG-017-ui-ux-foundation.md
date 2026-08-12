# GAG-017 — UI/UX 质感基线（Phase 0）

## 元数据

| 项 | 值 |
|----|-----|
| 分支 | `feat/GAG-017-ui-ux-foundation` |
| 依据 | `docs/UI-UX-OPTIMIZATION-PLAN.md` Phase 0；`docs/UI-UX-AUDIT-REPORT.md` P0 |
| 首选模型 | 任意可完成前端实现的模型 |
| 依赖 | GAG-002 设计系统已合并 |

## 目标

完成设计系统硬化与全局 Toast，使 v0.2 视觉与反馈达到可感知的产品质感，不改变业务 Bridge 契约与安全不变量。

## 范围（允许修改）

- `src/shared/theme/*`
- `src/shared/ui/*`（含新增 Icon / Toast）
- `src/shared/composables/*`（新增）
- `src/app/*`（挂载 Toast、AppShell 小修）
- `src/features/**` 中仅限：标题字号/Token 引用、StatusIcon 消费方无行为变化、Review 搜索改用 Input、banner class 合并、打开项目等结果级 Toast 接入
- `tests/**` 与 `docs/UI-UX-OPTIMIZATION-PLAN.md`、本任务说明书

## 不在范围

- 亮色主题切换
- TaskCenter 头部大重构（Phase 1）
- 面包屑 / 状态栏信息密度 / 首次引导 / Ctrl+K
- DesktopBridge、ACP、Git、Migration 变更
- 窗口最小高度变更

## 验收标准

1. `tokens.css` 含完整字号 scale、语义 heading、阴影、断点注释/变量、`radius-panel`、overlay/backdrop。
2. 页面主标题使用 `--heading-page`（Onboarding 展示标题可用 display）。
3. StatusIcon 与顶栏导航图标为 SVG，非依赖系统 Unicode 渲染。
4. Toast 可 success/error 弹出；宿主挂在应用根。
5. `font-weight: 650` 清除；`banner-stale` 与 `banner-warn` 合并或等价。
6. Review 搜索使用共享 Input。
7. `npm run typecheck`、`npm run lint`、相关单测通过。

## 测试

- `npm run typecheck`
- `npm run lint`
- `npm run test:node`（含 GAG-002 / GAG-017）
- `npm run test:ui`（Toast / 组件相关）
