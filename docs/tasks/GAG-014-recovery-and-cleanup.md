# GAG-014：恢复中心与安全清理

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-014 |
| 类型 | 恢复 / 数据保全 / 破坏性清理 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | DeepSeek V4 Pro |
| 推荐 reasoning effort | XHigh |

Luna 可补充既定诊断类型的 UI 快照和报告模板；Flash 可处理机械性状态文案。恢复决策、孤儿归属证明、删除和数据库修复不得交给低成本模型。

升级与审查：任何无法证明资源归属、恢复包不完整、数据库/磁盘/Git 三方不一致或删除范围不确定的情况必须 fail closed 并由 Sol 处理。D5 合并前异构旗舰审查。

## 2. 背景与目标

崩溃、断电、外部 Git 操作或磁盘问题可能留下中断会话、孤儿 Worktree、临时 Artifact 和未完成集成。本任务提供统一 Recovery Center，先盘点和保全，再允许修复或清理。

## 3. 需求映射

- PRD：FR-RECOVERY-001～003、FR-WORKTREE-004～005、FR-SESSION-004、FR-REVIEW-005、NFR-RELIABILITY-001～002、NFR-SECURITY-003～004。
- UI：UI-RECOVERY-001、UI-WORKTREE-001、UI-TASK-002。
- 技术：MOD-TASK-RUNTIME、MOD-WORKSPACE、MOD-ARTIFACTS、MOD-PERSISTENCE、ADP-FS、ADP-GIT-CLI。
- 前置：GAG-006、GAG-009～013。

## 4. 必读文档

- `AGENTS.md`：破坏性操作和任务授权。
- `01-PRD.md` 3.10、5.10、6。
- `02-UI-UX-DESIGN.md` 5.10、6～7。
- `03-TECHNICAL-DESIGN.md` 10～15。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 5。

## 5. 开始条件

- 各模块能报告受管资源 identity、状态与 owner。
- Worktree 删除和 integration abort 已提供安全原语，本任务只编排，不复制低层删除算法。
- 恢复包格式、校验哈希和默认存储位置已冻结。
- 无法识别来源的目录一律视为外部资源，不得删除。

## 6. 实现范围

- 启动/手动诊断扫描：中断任务、孤儿进程登记、Worktree 三方差异、临时集成、Artifact 临时文件、未完成 Migration/事务标记。
- `RecoveryIssue` 分类、严重度、证据和安全操作集合。
- 生成包含 manifest、Git refs、patch、未跟踪文件、数据库摘要和哈希的恢复包。
- 操作：标记已中断、重新登记、保留、显示位置、恢复会话、继续/中止集成、验证后清理。
- 清理计划预览、批准、逐项执行、失败隔离和可重复运行。
- `UI-RECOVERY-001` 诊断列表、证据、影响、推荐动作和结果报告。

## 7. 非范围与文件边界

非范围：操作系统级文件恢复、云备份、自动解决 Git 冲突、静默删除未知资源。

允许：

- `src-tauri/src/modules/task_runtime/**` 的 recovery orchestration
- `src-tauri/src/modules/workspace/**`、`artifacts/**`、`persistence/**` 的诊断接口绑定
- `src/features/worktrees/**` 或 `src/features/task-center/**` 的 Recovery Center
- 恢复 fixtures/tests 与必要 Migration

禁止：复制/绕过 GAG-011 的删除验证；直接递归删除未证明目标；把恢复包放在将被删除的唯一目录中。

## 8. Interface、状态与数据

`RecoveryService`：`scan`、`get_issue`、`prepare_action`、`execute_action`、`create_bundle`、`verify_bundle`、`list_history`。

Issue 状态：`detected -> assessed -> ready -> executing -> resolved|retained|failed|needs_manual_action`。重新扫描可创建新 revision，不覆盖旧证据。

`RecoveryActionPlan` 绑定 issue revision、资源 identities、canonical paths、expected Git/DB state、步骤、破坏性等级、审批证据和有效期。

SQLite：append-only 记录 scan、issue、action plan、bundle、步骤结果；不得用恢复流程改写历史事件。必要修复通过新事务和审计事件完成。

## 9. 恢复包最低内容

- `manifest.json`：版本、创建时间、应用版本、任务/session/worktree/integration IDs。
- Git：repo identity、相关 refs/SHA、status、tracked patch；可安全取得时包含 staged patch。
- 未跟踪文件：受大小/类型限制的归档与跳过清单。
- Artifact/数据库：元数据摘要与引用，不复制秘密。
- 每个文件的大小和 SHA-256；包自身校验结果。

恢复包必须存放在独立受管 recovery root；创建并验证成功后才允许破坏性清理。

推荐实施顺序：先实现只读 scan 与 issue revision，再实现恢复包创建/校验，然后实现单项非破坏性恢复，最后在 GAG-011 安全原语上编排清理并接入 UI；批量操作最后开放。

## 10. UI 与用户流

启动发现问题时显示非阻断横幅；高风险问题可阻止相关任务操作但不阻止查看。

Recovery Center 按“需立即处理/可安全延后/仅信息”分组。选择问题显示检测证据、当前数据、候选操作和风险。危险操作的确认页显示精确资源与恢复包状态。

批量清理只对相同、低风险、已验证策略开放；任何一项状态变化后跳过该项并报告，不扩大范围。

## 11. 安全不变量

- 先扫描、再计划、再重新验证、后执行；计划和执行间状态变化即拒绝。
- 破坏性步骤前必须有经过校验且位于独立位置的恢复包。
- 数据库记录、Git 列表和 canonical 文件路径至少满足文档定义的归属证明；不满足则仅允许保留/显示。
- 清理按单资源边界执行，单项失败不触发 broad cleanup。
- 任何未知 issue 类型、schema version 或 action 均 fail closed。
- 恢复操作不得重放不确定的 Agent 写请求。

## 12. 自动化测试

- 每类 issue 的检测、去重、revision 和安全操作集合测试。
- 崩溃点覆盖 bundle 创建、校验、Git remove、文件清理、DB 提交前后。
- 数据库/Git/磁盘各种不一致组合和篡改路径测试。
- 不完整/损坏/位于目标内的恢复包必须阻止删除。
- 多 issue 批处理中的状态变化和部分失败测试。
- UI 证据、确认、进度、重试和人工处理状态测试。

## 13. 手工验收

1. 模拟应用崩溃后的 running session，重启能识别并安全恢复/标记。
2. 制造孤儿 Worktree 和临时 integration，扫描能区分并提供正确操作。
3. 故意损坏恢复包，删除被阻止。
4. 篡改数据库路径到受管根外，只允许显示/保留。
5. 清理成功后重新扫描无悬挂记录，历史报告仍可查看。

## 14. Definition of Done

- 诊断、证据、恢复包、计划、重新验证、执行和历史闭环完成。
- 无未知或未证明资源的删除路径。
- 故障注入、路径、安全、UI、Migration、Lint、类型检查和构建通过。
- 异构旗舰模型完成独立安全审查。
- 交付报告附问题矩阵、恢复包样例校验、破坏性操作证明和剩余风险。

## 15. 标准任务交付报告

包含 Task ID、实现/审查模型、reasoning、修改文件、issue/action 矩阵、Migration、恢复包格式、故障注入结果、手工证据、被拒绝的危险案例、已知限制。
