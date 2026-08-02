# GAG-002：Catppuccin Mocha Design System 与 App Shell

## 1. 任务元数据

- Task ID：GAG-002
- 类型：feat/ui
- 难度：D3
- 首选模型：Grok 4.5，reasoning `high`
- 备选模型：GPT-5.6 Terra，reasoning `high`
- 推荐 reasoning effort：High
- 前置任务：GAG-001
- 可并行：可与 GAG-003 并行；不得修改 Bridge 公共类型

Luna 或 Flash 可生成 Token 映射、Story/测试样板和重复 aria 标签；不能独立决定布局或可访问性。若同一验收项连续失败两次、出现跨 Feature 状态、Tauri 原生窗口问题或 200% 缩放不可用，升级 Terra；安全或跨层状态问题升级 Sol。

## 2. 目标

建立唯一 Catppuccin Mocha Design Token、可访问基础控件和三栏 App Shell。该任务只实现视觉骨架和静态交互，不连接真实 Grok/Task/Worktree 数据。

## 3. 规范映射

- PRD：NFR-ACCESSIBILITY-001/002、NFR-PERFORMANCE-001
- UI：第 2–4、7 节；主窗口布局
- 技术：`src/shared/theme`、`src/shared/ui`、`src/app`、`src/features` UI 外壳

## 4. 必读材料

- `docs/02-UI-UX-DESIGN.md` 全文
- `docs/03-TECHNICAL-DESIGN.md` 第 3 节依赖方向
- Catppuccin 官方 Mocha palette

开始条件：GAG-001 已通过构建，Vue/Tauri 入口与上游版本可追溯；字体、Monaco 和图标依赖已经确认许可证。若 UI 文档的 Token、尺寸或响应式规则与现有代码冲突，先提交差异报告，不自行选择新设计。

## 5. 实现范围

- CSS variables/TypeScript token 表与 Monaco theme 使用同一来源。
- 字体、字号、间距、圆角、边框、状态和 motion Token。
- 基础控件：Button、IconButton、Input、Textarea、Select、Dialog、Drawer、Tooltip、Badge、StatusIcon、EmptyState、ErrorState、Skeleton。
- 三栏 Shell：顶部栏、左任务栏、中主区、右 Inspector、底状态栏、Resizable/Collapse 行为。
- 1024/1200/1440 宽度和 200% 缩放降级。
- 键盘焦点、visible focus、ARIA label、reduced motion。
- 静态 UI route/fixture 展示所有控件状态。

## 6. 非范围

- 不实现业务 Store、真实任务列表、ACP、权限或 Git。
- 不加入第二主题或主题选择器。
- 不实现文件编辑器、交互式终端或动画装饰。

## 7. 允许修改

- `src/shared/theme/**`
- `src/shared/ui/**`
- `src/app/**` 中 Shell、routing 和视觉状态
- 与静态展示有关的 feature view 文件及测试
- Monaco theme adapter（若包已存在）

禁止修改 `src/bridge/**`、Rust Module、Migration 和 Tauri capabilities。

## 8. Interface 与状态

前端 UI Interface 至少包含：

```ts
type AppShellProps = {
  left: VNode;
  main: VNode;
  inspector?: VNode;
  inspectorOpen: boolean;
  statusBar?: VNode;
};
```

基础控件状态统一为 `default/hover/focus/active/disabled/loading/error`。状态颜色必须同时提供文本或图标。

## 9. 推荐实施顺序

1. 建立 Token 文件和 lint 规则，禁止 feature 硬编码 palette hex。
2. 实现 Typography、Button、Input、Dialog 等基础控件。
3. 实现三栏 Shell 和 resizer，保证主区最小宽度。
4. 实现 Drawer 降级和键盘导航。
5. 注册 Monaco Mocha theme。
6. 建立控件矩阵/静态演示路由和视觉快照。

## 10. 异常与安全

- 用户保存的异常栏宽必须 clamp 到允许范围。
- Inspector 内容缺失时自动折叠，不能留不可聚焦空面板。
- Dialog focus trap、Esc、恢复焦点必须正确。
- 禁止 `v-html` 渲染未清洗内容。

## 11. 自动化测试

- Token 完整性及官方 hex 精确匹配。
- 静态扫描 Feature CSS 不含 Mocha palette hex。
- Button/Dialog/Drawer/Resizable 的交互和键盘测试。
- 1024、1200、1440 viewport 快照。
- 200% 字体/浏览器缩放下关键操作仍存在。
- axe 或等价 a11y 检查无 critical/serious 问题。

## 12. 手工验收

- UI 与 `02-UI-UX-DESIGN.md` 三栏线框一致。
- Tab 可遍历全局栏、左栏、中栏、Composer slot、右栏。
- 仅用键盘能打开/关闭 Inspector 和 Dialog。
- Mocha 背景层级清晰，状态不依赖颜色。

## 13. Definition of Done

- Design Token 是唯一色彩来源，Monaco 与应用一致。
- 三栏在三种宽度及 200% 缩放可用。
- 基础控件覆盖全部状态并有测试。
- 没有业务 mock 泄漏到生产入口；静态 fixture 仅测试/开发可访问。
- 未修改任务外 Bridge/Rust 范围。

## 14. 数据、外部交互与 Migration 影响

- UI fixture 仅覆盖静态 shell、控件状态和页面 slot；不得模拟会影响业务判断的任务状态机。
- 本任务不调用 ACP、Git、文件系统或子进程，不创建 SQLite Migration；布局偏好如需持久化，只消费 GAG-003 未来契约或使用明确的内存 fallback。
- Renderer 不直接调用 Tauri；若静态壳需要 bootstrap 数据，必须通过可替换的 DesktopBridge mock。

## 15. 标准任务交付报告

报告必须包含：Task ID；实际主/辅助模型与 reasoning；升级原因；修改文件；Token 与组件清单；viewport/缩放矩阵；键盘与 axe 结果；测试、类型检查、Lint 和构建退出码；截图/视觉差异；未完成状态和已知限制。
