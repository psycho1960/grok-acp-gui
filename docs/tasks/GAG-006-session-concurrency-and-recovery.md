# GAG-006：会话并发、事件排序与恢复

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-006 |
| 类型 | 后端 / 并发 / 状态恢复 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | DeepSeek V4 Pro |
| 推荐 reasoning effort | XHigh |

Luna 或 Flash 仅可在状态机冻结后补充确定性测试、快照和文档。并发控制、事件去重、崩溃恢复与写入重放不得交给 Luna/Flash 独立完成。

升级规则：DeepSeek V4 Pro 如遇跨进程竞态、数据库与内存状态无法收敛或安全相关重放问题，升级 GPT-5.6 Sol。D5 合并前必须由不同旗舰模型独立审查。

## 2. 背景与目标

多个任务可并行运行，但同一会话中的事件必须有确定顺序。应用或 Grok Runtime 崩溃后，用户应看见已确认事件、明确的中断点和安全恢复选项，不能悄悄重放可能产生副作用的命令。

本任务在 `MOD-TASK-RUNTIME` 中实现 session supervisor、每会话邮箱、持久化事件日志、重连快照和恢复决策。

## 3. 需求映射

- PRD：FR-TASK-003～004、FR-SESSION-002～005、FR-RECOVERY-001～003、NFR-RELIABILITY-001～002。
- UI：UI-TASK-001、UI-TASK-002、UI-CONV-001、UI-RECOVERY-001。
- 技术：MOD-TASK-RUNTIME、MOD-PERSISTENCE、MOD-AGENT-RUNTIME；`session:snapshot`、`agent:event`、`task:status_changed`。
- 前置：GAG-003、GAG-004、GAG-005。

## 4. 必读文档

- 根 `AGENTS.md`：任务隔离、Plan 与安全、证据化交付。
- `01-PRD.md` 3.3～3.5、5.3～5.4、5.10、6。
- `02-UI-UX-DESIGN.md` 5.3～5.5、5.10、6～7。
- `03-TECHNICAL-DESIGN.md` 5～10、14～15。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 2、依赖图。

## 5. 开始条件

- Agent Runtime 能产生带 `session_id`、`sequence`、`correlation_id` 的稳定事件。
- Repository Interface 支持事务写入 session、event 和 task 状态。
- Bridge snapshot 与事件 DTO 已冻结。
- 如生产协议不能提供序列号，必须在技术评审中确认由本地接收顺序分配，禁止自行假设远端全序。

## 6. 实现范围

- 每会话串行邮箱，不同会话允许并行。
- 任务级最大并发和排队状态；并发数来自受验证设置。
- 事件去重、缺口检测、持久化后发布。
- Renderer 订阅后的 snapshot + delta 恢复协议。
- 应用重启时把不确定运行态转换为 `interrupted` 并生成恢复建议。
- session cancel、pause/recover 的幂等领域命令。
- 明确区分可安全重试的读取与禁止自动重放的副作用请求。

## 7. 非范围与文件边界

非范围：任务中心视觉、权限弹窗、Git Worktree、Squash、Artifact 预览。

允许修改：

- `src-tauri/src/modules/task_runtime/**`
- `src-tauri/src/modules/persistence/**` 中现有 Repository 实现和本任务已规划 Migration
- `src-tauri/src/bridge/**` 中既有契约绑定
- 并发/恢复 fixtures 与测试

禁止修改：Grok 协议 Adapter 的既有外部契约、`src/features/**`、Git Adapter、打包配置。Migration 必须新增，已合并 Migration 不得改写。

## 8. 状态与一致性规则

Task 状态必须与 PRD 完全一致：`draft -> preparing -> running -> waiting_permission|idle|interrupted|failed -> ready_for_review -> integrating -> conflicted|merged -> archived`。`waiting_permission` 通过 `wait_reason=permission|plan` 区分权限和 Plan 等待；不得新增 `waiting_plan`。`conflicted` 可在人工处理后回到 `ready_for_review`；`merged` 只能进入 `archived`。取消的是当前 Session/Turn：确认工作区完整时 Task 回到 `idle`，完整性不确定时进入 `interrupted`，不得新增 Task `cancelled` 状态。

Session 状态：`created -> starting -> active -> waiting -> stopping -> completed|failed|cancelled|interrupted`。

事件提交规则：验证序列与去重键 → 在同一事务中追加事件并更新聚合状态 → 提交 → 发布 Bridge 事件。不得先发布后落库。

恢复规则：应用启动时，数据库中的 `preparing/running/waiting_*` 若没有有效受管进程证明，统一标为 `interrupted`；用户明确选择后才能新建恢复 attempt。原 attempt 永不覆盖。

## 9. Interface 与数据影响

`TaskRuntime` 至少提供：`enqueue_task`、`start_session`、`accept_agent_event`、`cancel_session`、`get_snapshot`、`list_recovery_candidates`、`recover_session`。

DTO：`TaskSummary`、`SessionSnapshot`、`TimelineCursor`、`RecoveryCandidate`、`RecoveryDecision`、`ConcurrencyLimits`。

SQLite：如 GAG-004 尚未创建，新增只追加 Migration，包含 event 去重键、attempt 编号、last_sequence、interruption reason 和 recovery metadata；所有 schema 变化必须有前向 Migration 测试。

## 10. 推荐实施顺序

1. 写状态机和不变量的纯函数测试。
2. 实现每会话 mailbox 与全局 semaphore。
3. 实现事件事务、去重和缺口检测。
4. 实现 snapshot/cursor 与订阅重连。
5. 实现启动恢复扫描与新 attempt 机制。
6. 做故障注入：崩溃点覆盖事务前、事务后、发布前、发布后。

## 11. 异常与安全不变量

- 同一 session 的领域变更只能由其邮箱串行执行。
- 重复事件必须幂等；序列缺口不得静默跳过。
- 不同 session 的慢请求不能阻塞全局 UI 查询。
- 未确认是否执行的写操作不得自动重放。
- cancel 竞争完成事件时，只允许一个终态，以数据库提交顺序裁决并记录诊断。
- snapshot 与其后的 delta 必须以 cursor 划界，不能漏事件或双计数。
- 恢复失败不得破坏原事件和 Artifact 引用。

## 12. 自动化测试

- 状态机表驱动与 property tests。
- 10+ 并发 session 压力测试，验证单会话有序、跨会话并行。
- 重复、乱序、缺口、迟到终态事件测试。
- cancel/turn-completed、timeout/permission-response 等竞态测试。
- SQLite 事务故障注入和应用重启恢复测试。
- snapshot + delta 断线重连一致性测试。
- 禁止副作用自动重放的断言测试。

## 13. 手工验收

1. 同时启动多个任务，任务中心持续可交互。
2. 运行中强制结束应用并重启，原任务显示 `interrupted`。
3. 选择恢复，产生新的 attempt，旧时间线仍可查看。
4. 断开并重新打开会话视图，无丢失或重复消息。
5. 取消与完成近同时发生时，界面只出现一个稳定终态。

## 14. Definition of Done

- 状态、事务顺序、恢复语义和重放边界有代码级不变量及测试。
- 并发压力与故障注入测试稳定通过，无 flaky 重试掩盖。
- 所有 Migration 可从空库和上一版本升级。
- Sol 或 DeepSeek V4 Pro 的异构独立审查完成。
- 交付报告列出竞态矩阵、故障注入点、测试种子和已知限制。

## 15. 标准任务交付报告

必须包含 Task ID、模型与 reasoning、升级记录、状态图差异、Migration、修改文件、并发参数、自动化与手工证据、故障注入结果、审查人模型、剩余风险。
