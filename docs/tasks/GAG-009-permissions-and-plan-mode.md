# GAG-009：权限审批与 Plan Mode

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-009 |
| 类型 | 安全 / 跨层 / 状态机 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | DeepSeek V4 Pro |
| 推荐 reasoning effort | XHigh；策略变更使用 Max |

Luna/Flash 只能在策略和契约冻结后补充文案、固定枚举测试与 UI 快照。不得独立实现命令分类、批准令牌、Plan 写门禁或 fail-closed 逻辑。

升级规则：任何策略歧义、绕过路径、TOCTOU、跨 session 权限串用或文档/协议冲突必须立即由 GPT-5.6 Sol 处理；D5 合并前由不同旗舰模型进行威胁建模复核。

## 2. 背景与目标

Agent 可能请求执行命令、修改文件或访问敏感资源。本任务提供统一权限决策，并保证 Plan Mode 在批准前只允许只读行为。安全决策必须发生在后端执行边界，UI 只展示和提交用户选择。

## 3. 需求映射

- PRD：FR-PERMISSION-001～003、FR-PLAN-001～002、FR-SESSION-003、NFR-SECURITY-001～004。
- UI：UI-PERM-001、UI-PLAN-001、UI-CONV-001。
- 技术：MOD-TASK-RUNTIME、MOD-AGENT-RUNTIME、DesktopBridge `resolve_permission/resolve_plan` 与对应事件。
- 前置：GAG-003～006、GAG-008；Git/文件执行点由 GAG-011～013 接入同一 guard。

## 4. 必读文档

- `AGENTS.md`：Plan、Shell、Git、文件系统和升级规则。
- `01-PRD.md` 3.5、5.5～5.6、6.1。
- `02-UI-UX-DESIGN.md` 5.6～5.7、6～7。
- `03-TECHNICAL-DESIGN.md` 5～10、13～14。
- `04-AI-DEVELOPMENT-ROADMAP.md` GAG-009 依赖和 D5 审查要求。

## 5. 开始条件

- ACP 权限/Plan 事件已映射为内部 DTO；不确定字段语义必须先报告。
- Bridge 命令拥有 session、request、correlation 和版本字段。
- 已明确只读命令 allowlist；不存在“未知即只读”的默认分支。

## 6. 实现范围

- 权限请求生命周期、超时、拒绝、一次批准和受限持久批准。
- Plan 提案显示、批准、拒绝、请求修改以及版本绑定。
- 后端统一 `ExecutionGuard`，供进程、文件和 Git Adapter 调用。
- 命令/操作分类：read-only、write、destructive、unknown。
- 审批令牌绑定 session、operation digest、workspace、plan version、过期时间和单次消费状态。
- 权限与 Plan UI：影响范围、命令参数、路径、风险、默认拒绝和键盘操作。
- 审计事件记录决策元数据，不记录密钥或完整敏感参数。

## 7. 非范围与文件边界

非范围：实际 Git 合并/删除算法、ACP 编解码器重写、任意 Shell 终端。

允许：

- `src-tauri/src/modules/task_runtime/**` 中 permission/plan/guard
- `src-tauri/src/bridge/**` 的既定契约实现
- `src/features/conversation/**` 的权限/Plan 插槽
- 如已规划，可新建 `src/features/settings/**` 中权限策略页面
- 对应 tests/fixtures

禁止：绕过 Guard 直接执行的 Adapter 快捷路径；修改已合并 Migration；将任意 shell 或原始审批令牌暴露给 Renderer。

## 8. 状态、Interface 与数据

Permission：`requested -> approved_once|approved_scope|denied|expired|cancelled -> consumed`；只有 approved 状态可被消费一次。scope approval 必须是显式、受限规则，不得用字符串前缀泛化危险命令。

Plan：`draft -> proposed -> approved|rejected|revision_requested -> superseded|executing -> completed|failed`。新的 plan version 使旧批准失效。

`ExecutionGuard.authorize(OperationDescriptor, ExecutionContext)` 返回 `Allowed(ApprovalEvidence)` 或结构化 Denied/Pending；任何未知/缺失字段返回 Denied。

SQLite：新增 Migration（若 GAG-004 未预建）保存 plan version/hash、permission decision、scope、expiry、consumed_at 和审计摘要；不可保存秘密值。

## 9. UI 布局与用户流

`UI-PERM-001` 以阻断式卡片/弹层呈现：操作类别、规范化命令和参数、工作目录、读写路径、风险解释；主按钮遵循默认拒绝，不通过颜色单独表达危险。

`UI-PLAN-001` 显示版本、目标、步骤、预计写入和命令类别。批准必须绑定当前版本；计划更新后显示“批准已失效”。

流转：后端发出 pending → UI 聚焦待处理项 → 用户选择 → Bridge 提交带 expected_version 的决策 → 后端原子校验并返回结果 → UI 更新。关闭窗口、超时或 Bridge 断开均视为未批准。

## 10. 推荐实施顺序

1. 编写操作分类和 fail-closed 测试矩阵。
2. 实现审批令牌、状态机和持久化事务。
3. 在 Fake executor 上强制所有路径通过 Guard。
4. 实现权限与 Plan UI 及并发/过期反馈。
5. 接入 Agent Runtime，并为后续 Git/FS Adapter 提供稳定接口。
6. 完成威胁建模和绕过测试。

## 11. 安全不变量

- Plan 批准前禁止文件写入、写数据库业务数据、写 Git、启动非只读命令；仅允许声明过的只读探测。
- 缺失、未知、过期、版本不匹配或已消费批准全部拒绝。
- 审批不能跨 session、workspace、operation digest 或 plan version 复用。
- UI 状态不能作为授权来源；后端在真正 I/O 前再次校验。
- 参数数组和规范化 cwd 参与 operation digest；批准后参数变化必须重新审批。
- 应用崩溃后一次批准不恢复为可用；持久 scope 按显式规则重新判定。
- 拒绝/超时不触发隐式降级操作。

## 12. 自动化测试

- 操作分类表：读、写、破坏性、未知、混合参数和路径逃逸。
- 所有缺失字段、旧版本、重复消费、跨 session/workspace 复用测试。
- Plan 更新使批准失效、拒绝和 revision 流测试。
- UI 超时、窗口关闭、双击批准、多个 pending 请求测试。
- Adapter 绕过扫描/集成测试，证明无未受 Guard 保护的写路径。
- 日志和数据库敏感字段检查。

## 13. 手工验收

1. Plan 未批准时尝试写文件/运行写命令，均被拒绝。
2. 批准特定操作后更改一个参数，必须再次请求权限。
3. 拒绝、超时、关闭应用后均不执行。
4. 新 Plan 版本出现时旧批准立即失效。
5. 仅键盘可以检查详情、拒绝或批准，默认焦点不在危险按钮。

## 14. Definition of Done

- 所有执行 Adapter 可接入统一 Guard，且默认拒绝。
- 权限/Plan 状态机、版本和批准证据有事务与测试。
- 完成独立旗舰模型安全审查和绕过清单。
- 测试、Lint、类型检查、构建通过。
- 交付报告附威胁模型、策略矩阵、模型升级记录和审查结论。

## 15. 标准任务交付报告

报告包含 Task ID、实现/审查模型、reasoning、修改文件、策略矩阵、Migration、批准证据设计、自动化/手工验证、发现并封堵的绕过路径、剩余风险。
