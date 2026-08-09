# GAG-011：Worktree 生命周期

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-011 |
| 类型 | Git / 文件系统 / 安全关键生命周期 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | DeepSeek V4 Pro |
| 推荐 reasoning effort | XHigh |

Luna/Flash 仅可补充固定 Git fixture、文案和纯显示层测试。Worktree 创建、路径验证、删除、恢复包与命令参数不得由低成本模型独立实现。

升级与审查：DeepSeek V4 Pro 遇到 Windows 路径、竞态、数据保全或 Git 状态歧义时升级 Sol；合并前必须由不同旗舰模型复核所有创建/删除命令与路径证明。

## 2. 背景与目标

每个执行任务必须在独立 Worktree 中工作，以隔离并行修改。创建和清理涉及 Git 与文件系统，错误实现可能删除用户数据。本任务建立受管根、命名、创建、检测、关闭和安全删除流程。

## 3. 需求映射

- PRD：FR-PROJECT-001～004、FR-TASK-001～004、FR-WORKTREE-001～005、FR-RECOVERY-001～003、NFR-SECURITY-003～004。
- UI：UI-PROJECT-001、UI-WORKTREE-001、UI-RECOVERY-001。
- 技术：MOD-WORKSPACE、ADP-GIT-CLI、ADP-FS、ExecutionGuard。
- 前置：GAG-003、GAG-004、GAG-006、GAG-009。

## 4. 必读文档

- `AGENTS.md`：Git、Worktree、破坏性操作、任务范围。
- `01-PRD.md` 3.2～3.3、3.8～3.10、5.2～5.3、5.8、5.10。
- `02-UI-UX-DESIGN.md` 5.2、5.9～5.10、6。
- `03-TECHNICAL-DESIGN.md` 5、10、13～15。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 4。

## 5. 开始条件

- GAG-010B 已完成模式与持久化工作区策略联动及缺失 Worktree 的 fail-closed 前置约束。
- Project/Task/Worktree schema 和 Bridge DTO 已稳定。
- `ExecutionGuard` 可对 Git/FS 操作授权。
- 测试仓库 fixtures 包含普通、dirty、detached、路径含空格/中文、嵌套 Git 和重解析点情形。

## 6. 实现范围

- 验证 Git 仓库、解析 canonical repo root 与 common git dir。
- 受管 Worktree 根配置、目录命名和唯一性。
- 创建 `gag/GAG-###-slug` 分支与 Worktree；幂等检测和失败回滚。
- 列出、健康检查、识别 missing/locked/orphaned/dirty/merged 状态。
- 关闭任务后的保留、归档、生成恢复包后删除。
- Worktree 管理 UI：路径、分支、dirty、磁盘占用、关联任务和安全操作。
- 所有 Git 命令使用可审计参数数组、明确 cwd、超时和输出上限。

## 7. 非范围与文件边界

非范围：Diff 选择、Checkpoint、Squash 集成算法、远程 push。

允许：

- `src-tauri/src/modules/workspace/**`
- `src-tauri/src/adapters/git_cli/**`
- `src-tauri/src/adapters/filesystem/**` 中 Worktree 路径操作
- `src/features/worktrees/**`
- 对应 fixtures/tests 和必要的新 Migration

禁止：任意 shell 字符串、删除非受管目录、改写已有 Migration、在 Renderer 直接执行 Git。

## 8. Interface、状态与数据

`WorkspaceService`：`inspect_repository`、`create_managed_worktree`、`list_worktrees`、`inspect_worktree`、`prepare_removal`、`remove_managed_worktree`、`reconcile_registry`。

Worktree 状态：`allocating -> ready -> active -> closing -> archived|removed`；异常态 `creation_failed`、`missing`、`dirty`、`orphaned`、`quarantined`。删除只允许从经过重新检查的 closing/archived 路径进入 removed。

SQLite：记录 repo identity、canonical path、worktree relative path、branch、task、created_at、last_verified_at、state、recovery_bundle_id。文件系统不是唯一真相；启动时与 `git worktree list --porcelain` 对账。

## 9. 创建与删除算法

推荐实施顺序：先实现只读仓库/路径检查与 fixture，再实现创建和对账，随后实现恢复包，最后接入受 Guard 保护的删除和 UI；删除能力不得早于路径逃逸与恢复包失败测试。

创建：验证 repo → 获取锁/防重复 → 计算受管路径 → 验证路径父级 → 创建分支/Worktree → 校验 HEAD 和注册信息 → 事务写库。任一步失败时只回滚本次生成且可证明归属的资源。

删除前：重新 canonicalize → 验证属于配置的 managed root 且与数据库、Git 列表、task ID 一致 → 检测 dirty/untracked → 生成 manifest、patch、未跟踪文件归档和元数据的恢复包 → 校验恢复包 → 用户确认 → Git remove → 清理登记。验证失败必须拒绝。

不使用递归删除处理不确定路径；不把环境变量、glob 或用户字符串直接作为删除目标。

## 10. UI 与流转

`UI-WORKTREE-001` 列表展示关联任务、路径、分支、状态、dirty 和大小；详情显示最近校验、可恢复性与操作记录。

删除流：点击清理 → 后端 prepare 返回最新风险摘要 → dirty 时默认提供保留/打开/生成恢复包，不默认删除 → 二次确认显示准确绝对路径 → 执行 → 返回恢复包位置和结果。

## 11. 安全不变量

- 只有数据库登记、Git 列表确认且 canonical path 位于 managed root 的目录可删除。
- repo root、managed root、磁盘根、用户主目录和其祖先绝不能成为删除目标。
- 重解析点/符号链接目标必须验证，不因词法前缀相同而信任。
- 删除前后分别获取/验证任务锁，防止运行任务与清理竞态。
- dirty 或 untracked 内容未成功生成恢复包时禁止删除。
- 命令输出和日志需限制大小并脱敏路径中的用户名。

## 12. 自动化测试

- Git 创建/失败回滚/重复创建/分支冲突测试。
- 空格、中文、长路径、大小写、UNC（支持时）测试。
- `..`、同前缀目录、symlink/junction、嵌套 repo、篡改数据库路径逃逸测试。
- dirty/untracked、locked、missing、orphaned 对账测试。
- 删除时运行任务竞态和锁测试。
- 恢复包不完整/校验失败时拒绝删除测试。
- UI 风险摘要、二次确认和失败恢复测试。

## 13. 手工验收

1. 从含空格/中文路径的 repo 创建任务 Worktree。
2. 制造未提交与未跟踪文件，清理必须先生成可检查恢复包。
3. 篡改路径指向受管根外，删除被明确拒绝。
4. Git 外部删除 Worktree 后，应用识别 missing 并提供对账。
5. 正常清理后 Git 列表、磁盘和数据库一致。

## 14. Definition of Done

- 创建、登记、检查、对账、恢复包和删除闭环完整。
- 路径证明与破坏性操作有独立旗舰模型审查。
- 故障注入、安全、集成、UI、Lint、类型检查和构建通过。
- 无任何任意 shell 或未经 Guard 的 Git/FS 写路径。
- 交付报告提供实际命令数组示例、路径测试矩阵和恢复证据。

## 15. 标准任务交付报告

包含 Task ID、实现/审查模型、reasoning、修改文件、Git 命令清单、路径不变量、Migration、恢复包格式、测试证据、手工验收、未解决平台限制。
