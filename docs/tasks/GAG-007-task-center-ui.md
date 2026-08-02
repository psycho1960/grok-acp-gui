# GAG-007：Task Center UI

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-007 |
| 类型 | Renderer / Vue / 状态展示 |
| 难度 | D3 |
| 首选模型 | GPT-5.6 Terra |
| 备选模型 | Grok 4.5 |
| 推荐 reasoning effort | Medium；涉及跨视图状态错乱时提高到 High |

Luna 可补充已确定组件模式下的 Story、快照、空状态文案和测试。Flash 可执行机械性组件拆分。若发生任务状态与会话状态跨层不一致、Bridge 契约需变更或并发更新覆盖，升级 GPT-5.6 Sol。

## 2. 背景与目标

Task Center 是用户理解“当前在做什么、等待什么、哪些任务失败”的主入口。本任务实现任务分组、筛选、状态摘要、并发可视化和任务详情抽屉，不包含对话时间线本体。

## 3. 需求映射

- PRD：FR-PROJECT-003、FR-TASK-001～004、FR-SESSION-005、NFR-PERFORMANCE-001、NFR-ACCESSIBILITY-001～002。
- UI：UI-TASK-001、UI-TASK-002；App Shell 中央工作区和右侧检查器。
- 技术：Renderer `features/task-center`；只调用 DesktopBridge `list_tasks/get_task_snapshot/cancel_task` 并订阅 `task:status_changed`。
- 前置：GAG-002、GAG-003；联调依赖 GAG-006。

## 4. 必读文档

- `AGENTS.md`：Renderer/Bridge 边界、Design Token 和授权文件。
- `01-PRD.md` 3.2～3.4、5.2～5.4。
- `02-UI-UX-DESIGN.md` 3～7，重点 5.3、5.4。
- `03-TECHNICAL-DESIGN.md` 3～6、9。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 3 与共享文件说明。

## 5. 开始条件

- App Shell、主题、基础组件已由 GAG-002 提供。
- GAG-003 DTO 与 mock DesktopBridge 可用。
- 状态标签、排序规则和文案已在 UI 文档冻结。
- 若 Bridge 缺字段，只能提交契约变更提案，不能在 Renderer 拼接后端领域状态。

## 6. 实现范围

- Task Center 路由、Pinia store、查询与事件合并。
- 按运行中、等待处理、已完成、失败/中断分组。
- 状态、项目、更新时间筛选及关键词搜索。
- 任务卡片：名称、项目、状态、阶段、持续时间、最近事件、待处理徽标。
- 任务详情抽屉：会话摘要、Worktree 摘要占位契约、错误与恢复入口。
- loading、empty、offline、stale、error 和大列表虚拟化状态。
- 键盘导航、焦点恢复、ARIA live 的状态更新策略。

## 7. 非范围与文件边界

非范围：对话/工具渲染、权限与 Plan 弹层、后端状态机、Git 操作。

允许：`src/features/task-center/**`、对应 store/tests/stories；如有必要，仅可在 `src/app/**` 增加已规划路由入口。

禁止：`src-tauri/**`、`src/bridge/**` 契约定义、Design Token 原值、其他 feature 内部代码。

## 8. UI 布局与流转

`UI-TASK-001`：顶部含标题、计数、搜索和筛选；主体为可虚拟化列表；卡片主操作是打开任务，次操作依据状态显示取消或恢复。

`UI-TASK-002`：桌面宽度以右侧抽屉呈现，窄窗口使用全屏覆盖层。抽屉固定显示任务状态、开始/更新时间、会话 attempt；危险操作不得成为默认焦点。

用户流：进入 Task Center → 加载 snapshot → 应用本地筛选 → 接收增量事件 → 选择任务 → 打开详情/会话/恢复。路由参数必须可深链，刷新后恢复选择。

## 9. 数据与状态规则

- 消费的 DesktopBridge Interface 固定为 `list_tasks/get_task_snapshot/cancel_task` 和 `task:status_changed`；DTO 为 `TaskSummary`、`TaskSnapshot`、`TaskCapability`、`TimelineCursor`。缺字段只能显示兼容错误，不在 UI 猜测。
- Store 保留服务端 `version/cursor`；旧事件不得覆盖新 snapshot。
- 列表排序：需处理优先，其次运行中，再按最近更新时间降序；同值按 Task ID 稳定排序。
- 组件不得从文案推导状态；只使用枚举和 capability flags。
- 乐观更新仅用于无副作用 UI 偏好；cancel 等命令必须等待后端确认。
- SQLite 与 ACP 无直接影响；所有数据经 DesktopBridge。
- 安全边界：任务标题、错误摘要和路径都按不可信文本渲染；Renderer 不拼接命令、不打开任意路径，也不把 capability flag 当作后端授权证明。

## 10. 推荐实施顺序

1. 基于 mock Bridge 建 store 契约和事件归并测试。
2. 完成响应式骨架、分组和筛选。
3. 完成详情抽屉、深链和焦点管理。
4. 补齐所有异步状态和键盘操作。
5. 接入 GAG-006 真正 snapshot/event，验证乱序防护。

## 11. 异常与可访问性

- Bridge 断开时保留最后 snapshot，标记 stale，不伪装为实时。
- 单个任务数据异常应隔离为错误卡片，不使整个列表白屏。
- 状态变化通过节流的 ARIA live 播报，流式事件不得逐字打扰。
- 色彩不是唯一状态信号；徽标包含图标与文字。
- 取消操作需确认当前状态，并对已终态响应做无害提示。

## 12. 自动化测试

- Store snapshot + delta、旧版本、重复事件和断线测试。
- 分组、排序、搜索、筛选与深链组件测试。
- loading/empty/error/stale/大量任务视觉与交互测试。
- 键盘遍历、抽屉焦点陷阱/恢复、ARIA 名称测试。
- Playwright：创建 mock 任务、状态变化、筛选、打开详情、取消失败。

## 13. 手工验收

1. 1280×720、1440×900 和高 DPI 下无关键操作遮挡。
2. 只用键盘完成筛选、选择、打开和关闭详情。
3. 模拟 500 个任务，滚动平滑且状态更新不跳位。
4. Bridge 离线时显示 stale 与重试入口。
5. 任务从 running 进入 waiting/completed 时分组正确更新。

## 14. Definition of Done

- UI-TASK-001/002 的布局、状态和流转完整。
- Renderer 只使用 DesktopBridge，不复制领域规则。
- 单元、组件、E2E、类型检查、Lint 和构建通过。
- 交付报告含截图尺寸、无障碍检查、性能样本、模型使用和变更文件。

## 15. 标准任务交付报告

列出 Task ID、模型/reasoning、辅助模型、修改文件、契约消费点、覆盖状态、测试命令与结果、截图/录屏证据、可访问性结论、已知限制。
