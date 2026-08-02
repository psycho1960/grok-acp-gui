# GAG-008：对话与工具时间线

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-008 |
| 类型 | Renderer / 流式事件 / 复杂交互 |
| 难度 | D4 |
| 首选模型 | Grok 4.5 |
| 备选模型 | GPT-5.6 Sol |
| 推荐 reasoning effort | High |

Luna 或 Flash 可补齐固定事件种类的渲染测试、快照和文案；不得独立设计事件归并、流式一致性或工具安全展示。同一验收项连续失败两次先升级 Terra；出现未知 Grok 事件、跨层数据缺口、流式乱序或性能/内存问题时升级 GPT-5.6 Sol。

## 2. 背景与目标

用户需要在一个连续时间线中理解提问、Agent 推理进度、工具调用、结果、错误和 Artifact，而不是阅读终端日志。本任务把稳定的内部 `AgentEvent` 映射为可扫读、可折叠、可恢复定位的会话界面。

## 3. 需求映射

- PRD：FR-SESSION-001～005、FR-PERMISSION-001、FR-PLAN-001、FR-IMAGE-001～003、NFR-PERFORMANCE-001～002、NFR-ACCESSIBILITY-001～002。
- UI：UI-CONV-001，并预留 UI-PERM-001、UI-PLAN-001、UI-ARTIFACT-001 的插槽。
- 技术：`features/conversation`、DesktopBridge `get_session_snapshot/send_message/cancel_request` 与 `agent:event`。
- 前置：GAG-002、GAG-003、GAG-006；Artifact 真预览由 GAG-010 完成。

## 4. 必读文档

- `AGENTS.md` 的 Bridge、日志、Plan 与安全边界。
- `01-PRD.md` 3.4～3.6、5.4～5.7。
- `02-UI-UX-DESIGN.md` 3～7，重点 5.5～5.8。
- `03-TECHNICAL-DESIGN.md` 5～9、12、14。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 3。

## 5. 开始条件

- `AgentEvent` 联合类型、cursor 与 snapshot 语义稳定。
- App Shell、排版、代码块和状态组件可用。
- 必须拥有每类事件 fixture；未知事件必须有明确 fallback。

## 6. 实现范围

- 会话路由、store、snapshot + delta 合并。
- 用户消息、Assistant 流式文本、工具卡片、错误、系统状态、计划/权限/Artifact 插槽。
- 工具调用按 correlation 分组，显示状态、持续时间、受控输入摘要和结果摘要。
- 长会话虚拟化、增量渲染、自动滚动与“回到底部”行为。
- Composer：输入、发送、停止、禁用原因、草稿恢复。
- 深链到 event，复制安全摘要，展开/折叠工具详情。
- 对未知事件提供可诊断但不泄密的兼容卡片。

## 7. 非范围与文件边界

非范围：ACP 原始协议解析、权限决策、Plan 写入门禁、文件预览解码、Git Diff。

允许：`src/features/conversation/**`、相关 store/tests/stories；仅通过公共组件扩展 `src/shared/ui/**`，不得复制 Design Token。

禁止：`src-tauri/**`、`src/bridge/**` 契约定义、权限/Artifact/Review feature 内部实现。

## 8. UI 布局与用户流

顶栏显示任务、attempt、运行状态和停止操作；主体为时间线；底部 Composer 固定但不遮挡最后一项。工具卡片默认显示名称、阶段、耗时和摘要，敏感参数默认隐藏。

自动滚动规则：用户位于底部阈值内时跟随；用户向上阅读后停止跟随并显示未读计数；点击回到底部恢复。切换会话需保存各自滚动锚点。

发送流：输入 → 本地校验 → Bridge 命令 → 后端确认创建 request → 清空草稿。失败时保留文本并显示可重试原因，不重复发送已确认请求。

## 9. 事件归并规则

- UI Interface 为 `ConversationTimeline(items, cursor, status)`、`ToolCard(toolCall)`、`Composer(capabilities, draft)` 和权限/Plan/Artifact slots；领域 DTO 只能来自 DesktopBridge 的 `SessionSnapshot` 与 `AgentEvent` 联合类型。
- 以 `(session_id, sequence)` 去重，以 snapshot cursor 划分历史和增量。
- `assistant_delta` 仅追加到匹配 request/message；完成后冻结正文。
- 工具事件按 `tool_call_id` 归并；迟到 update 不得把 completed 改回 running。
- 未知事件显示类型、时间和安全摘要，不显示原始 JSON 默认值。
- 工具输入、命令、路径和结果遵循后端 `redaction` 标记；前端不得自行“解密”。
- SQLite、ACP、Git 均无直接调用。

## 10. 推荐实施顺序

1. 建立所有事件 fixtures 与归并 reducer 测试。
2. 实现静态时间线和工具卡片状态。
3. 实现流式归并、Composer、停止操作。
4. 实现虚拟化、滚动锚点和深链。
5. 接入权限、Plan、Artifact 的稳定 slot contract。
6. 做长会话性能与可访问性测试。

## 11. 异常、安全与性能

- 10,000 条事件下不得一次创建全部 DOM 节点。
- 高频 delta 需按帧/短窗口批处理，但不得更改字符顺序。
- Renderer 不记录完整 prompt、工具参数或文件内容到 console。
- Markdown/代码渲染必须禁用危险 HTML，并清洗链接协议。
- 复制操作默认复制可见脱敏内容；复制原文需明确用户动作且受权限策略允许。
- Bridge 离线时 Composer 禁用并保留草稿。

## 12. 自动化测试

- reducer 的重复、乱序、迟到、未知事件和 snapshot cursor 测试。
- 流式文本、工具生命周期、错误与终止组件测试。
- XSS/危险链接/超长无空格文本安全测试。
- 10,000 事件虚拟化和 delta burst 性能基准。
- 键盘、屏幕阅读器名称、焦点与 reduced-motion 测试。
- Playwright：发送、停止、向上滚动、未读计数、刷新恢复定位。

## 13. 手工验收

1. 正常对话、多个并行工具、错误工具均能清晰阅读。
2. 向上阅读时新内容不抢滚动位置。
3. 刷新后回到同一会话和附近事件。
4. 粘贴恶意 Markdown/HTML 不执行脚本或危险 URL。
5. 长会话滚动、展开卡片和流式输出保持流畅。

## 14. Definition of Done

- UI-CONV-001 全部布局、状态和交互完成。
- 每种已知 `AgentEvent` 有明确渲染或委托插槽，未知事件有安全 fallback。
- 性能、安全、组件、E2E、类型检查和构建通过。
- 交付报告说明事件映射、性能数据、脱敏策略、模型与升级情况。

## 15. 标准任务交付报告

包含 Task ID、模型/reasoning、修改文件、事件覆盖表、Bridge 调用、性能样本、自动化/手工结果、安全验证、截图证据、已知限制与后续依赖。
