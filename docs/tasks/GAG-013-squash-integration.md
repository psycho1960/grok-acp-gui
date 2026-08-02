# GAG-013：Squash 集成

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-013 |
| 类型 | Git / 集成 / 冲突隔离 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | DeepSeek V4 Pro |
| 推荐 reasoning effort | XHigh |

Luna/Flash 只能补充成功路径 UI 快照、固定 fixture 和交付文档。目标分支更新、临时集成 Worktree、冲突隔离、提交和回滚不得由低成本模型独立实现。

升级与审查：DeepSeek V4 Pro 实现后必须由 Sol 复核；Sol 实现后由 DeepSeek V4 Pro 或另一旗舰模型独立审查。任何会改写目标分支、删除用户提交或自动解决冲突的提案必须停止并升级。

## 2. 背景与目标

用户需要把任务分支的一组 Checkpoint 以一个 Squash commit 集成到目标分支。集成不能污染主工作树，不能在目标分支已变化时悄悄覆盖，也不能在冲突时留下半完成状态。

本任务固定使用临时集成 Worktree：从经过确认的目标 HEAD 创建隔离工作区，在其中执行 squash、验证、提交，再以 compare-and-swap 方式更新目标引用。

## 3. 需求映射

- PRD：FR-REVIEW-003～005、FR-WORKTREE-004～005、FR-RECOVERY-001～003、NFR-SECURITY-003～004、NFR-RELIABILITY-002。
- UI：UI-REVIEW-001、UI-RECOVERY-001、UI-WORKTREE-001。
- 技术：MOD-WORKSPACE、ADP-GIT-CLI、ADP-FS、ExecutionGuard、临时 integration Worktree 算法。
- 前置：GAG-009、GAG-011、GAG-012。

## 4. 必读文档

- `AGENTS.md`：Conventional Commits、Rebase、Squash、Git 安全。
- `01-PRD.md` 3.9～3.10、5.9～5.10、6。
- `02-UI-UX-DESIGN.md` 5.9～5.10、6～7。
- `03-TECHNICAL-DESIGN.md` 10、13～15。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 4 和 D5 门禁。

## 5. 开始条件

- 来源任务 Worktree、Checkpoint、目标 repo identity 均健康。
- GAG-011 能安全创建/删除临时受管 Worktree。
- GAG-012 能提供来源 commit range 和选择 manifest。
- 当前目标 HEAD、工作树状态、是否允许快进引用更新有明确产品规则。
- 权限与 Plan 已明确批准本次 source、target、commit message 与操作摘要。

## 6. 实现范围

- 集成预检：来源/目标、共同祖先、目标 HEAD、dirty/linked worktree、进行中的 Git 操作。
- 创建唯一临时 integration Worktree 和临时分支。
- 在临时环境基于目标 HEAD 执行 `merge --squash` 或等价受控算法。
- 展示冲突清单并中止自动集成；不自动解决冲突。
- 运行配置的验证命令，生成一个 Conventional Commit。
- 目标 ref 的 compare-and-swap 更新；目标改变时拒绝发布。
- 完成/失败后的审计、恢复包和临时资源清理。
- Review UI 中的集成预览、确认、进度和结果。

## 7. 非范围与文件边界

非范围：自动冲突解决、force push、远程仓库发布、改写已有共享提交、删除来源任务分支。

允许：

- `src-tauri/src/modules/workspace/**` 中 integration/squash
- `src-tauri/src/adapters/git_cli/**`
- `src/features/review/**` 的集成 UI
- 必要的 integration Repository/Migration、fixtures/tests

禁止：在用户主工作树执行 merge/reset/clean；通过 shell 字符串运行 Git；自动更新目标 ref 而不校验 expected old SHA。

## 8. Interface、状态与数据

`IntegrationService`：`prepare_squash`、`start_squash`、`get_integration_status`、`abort_integration`、`publish_integration`、`cleanup_integration`。

状态：`draft -> preflight -> staging -> conflicted|validating -> ready_to_publish -> publishing -> completed`；失败态 `preflight_failed`、`validation_failed`、`publish_rejected`、`cleanup_required`、`aborted`。只有 `ready_to_publish` 可进入 publishing。

`IntegrationPlan` 绑定 repo、source tip/range、target ref、expected target SHA、commit message、validation commands digest、approval evidence。任意值变化使审批失效。

SQLite：append-only Migration（如未预建）保存 integration attempt、source/target SHA、temporary worktree ID、状态、冲突摘要、验证结果、result commit 和 recovery bundle。

## 9. 固定算法

下列固定算法同时是推荐实施顺序：先逐步实现并验证 preflight/隔离 Worktree/squash/validation，再开放 CAS publish，最后接入恢复清理和 UI；发布能力不得早于目标 ref 竞态测试。

1. 获取 repo 集成锁；读取并冻结 `expected_target_sha` 与 `source_tip_sha`。
2. 验证来源包含所选 Checkpoint、目标 ref 未被占用于不安全状态。
3. 从 expected target SHA 创建临时受管 Worktree/branch。
4. 在临时 Worktree 执行 squash；冲突时保留隔离现场与清单，不接触目标 ref。
5. 无冲突则校验 staged tree 与预览，执行验证命令并提交。
6. 再次确认目标 ref 仍为 expected SHA；使用 Git 原子 ref 更新机制发布 result commit。
7. 记录 receipt，再按 GAG-011 安全流程清理临时 Worktree；清理失败不改变已成功发布事实。

## 10. UI 流程

Review 中选择“Squash 集成” → 显示来源提交、目标分支、预计文件、提交信息与验证命令 → 明确批准 → 展示阶段进度。

冲突时显示文件清单、临时 Worktree 路径和“打开以人工处理/中止并保留恢复包”；不得提供默认自动解决。目标 HEAD 已变化时提示重新预检，不允许继续使用旧审批。

成功页显示新 commit SHA、目标 ref、验证结果、来源 Worktree 保留状态和临时清理状态。

## 11. 安全不变量

- 主工作树和其 index 在整个流程前后保持不变。
- 所有集成发生在经过验证的临时受管 Worktree。
- source/target SHA、命令、cwd 和 commit message 与批准证据完全匹配。
- 目标引用只允许从 expected SHA 原子更新到 result SHA；变化时 fail closed。
- 冲突或验证失败不得更新目标 ref。
- abort 不执行 broad reset/clean 于不确定路径；只处理本 attempt 可证明拥有的资源。
- 不 force push、不删除来源分支、不覆盖用户提交。

## 12. 自动化测试

- 无冲突 squash、空变更、多个 Checkpoint、rename/binary 测试。
- 内容冲突、rename/delete、submodule 和 mode 冲突隔离测试。
- 预检后目标分支前进、来源 tip 改变、并发两次集成测试。
- 验证命令失败、commit 失败、ref update 失败、应用崩溃点故障注入。
- 断言主工作树 HEAD/index/files 完全不变。
- 临时 Worktree 路径逃逸和清理恢复包测试。
- UI 预览、审批失效、冲突和成功状态测试。

## 13. 手工验收

1. 将含多个 Checkpoint 的任务分支集成为一个 commit。
2. 制造冲突，确认目标分支无变化且隔离现场可检查。
3. 预检后从外部推进目标分支，发布被拒绝。
4. 验证命令失败时不更新目标 ref。
5. 成功后主工作树无文件或 index 变化，临时资源清理可审计。

## 14. Definition of Done

- 固定算法、状态机、CAS 发布与隔离恢复完整实现。
- 所有竞态、冲突、故障注入和主工作树不变性测试通过。
- 完成异构旗舰模型独立审查。
- Git 命令、权限、Migration、UI、Lint、类型检查和构建通过。
- 交付报告包含 source/target/result SHA、命令参数、审查结论与恢复路径。

## 15. 标准任务交付报告

包含 Task ID、实现/审查模型、reasoning、修改文件、固定算法偏差、Git 命令数组、Migration、竞态/冲突矩阵、测试与手工证据、恢复包、剩余风险。
