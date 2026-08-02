# GAG-012：Diff、文件选择与 Checkpoint

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-012 |
| 类型 | Git / Review / 一致性 |
| 难度 | D4 |
| 首选模型 | DeepSeek V4 Pro |
| 备选模型 | GPT-5.6 Sol |
| 推荐 reasoning effort | High |

Luna 可在 Git 语义冻结后补充文件类型快照、UI 文案和已有模式下的测试。Flash 可做确定性 DTO/组件生成。遇到索引竞态、部分选择误提交、二进制/重命名语义或跨层状态不一致时升级 GPT-5.6 Sol。

## 2. 背景与目标

用户需要检查 Agent 改动，并把选定文件形成可追踪的 Checkpoint。本任务实现 status/diff、文件选择、暂存与提交的一致闭环，避免把非选定或用户原有改动误纳入。

## 3. 需求映射

- PRD：FR-REVIEW-001～004、FR-WORKTREE-003、NFR-SECURITY-003、NFR-RELIABILITY-002。
- UI：UI-REVIEW-001、UI-WORKTREE-001。
- 技术：MOD-WORKSPACE、ADP-GIT-CLI、`features/review`、ExecutionGuard。
- 前置：GAG-003、GAG-004、GAG-009、GAG-011。

## 4. 必读文档

- `AGENTS.md`：任务范围、Git 参数数组、提交规范。
- `01-PRD.md` 3.8～3.9、5.8～5.9。
- `02-UI-UX-DESIGN.md` 5.9、6～7。
- `03-TECHNICAL-DESIGN.md` 5、10、13～14。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 4。

## 5. 开始条件

- Worktree identity、任务锁与 Git Adapter 可用。
- 任务分支不与用户主工作树共享 index。
- Diff/Checkpoint DTO 已在 Bridge 冻结。
- 已定义 Checkpoint commit message：`type(scope): ... [GAG-###]` 或文档指定的等价 Conventional Commit。

## 6. 实现范围

- 获取 porcelain status、文件元数据、文本/二进制/重命名 Diff。
- Review UI：文件树、状态、Diff viewer、搜索、空/加载/过大/二进制状态。
- 用户按文件选择纳入 Checkpoint；选择与当前 file identity/version 绑定。
- 创建 Checkpoint：重新检查 → 精确暂存选定路径 → 校验 index → commit → 返回 commit/树摘要。
- 冲突、submodule、symlink、case-only rename 和超大 Diff 的明确行为。
- Checkpoint 历史和与任务/attempt 的关联。

## 7. 非范围与文件边界

非范围：按 hunk 暂存、交互式 rebase、Squash 到目标分支、远程 push。

允许：

- `src-tauri/src/modules/workspace/**` 中 review/checkpoint
- `src-tauri/src/adapters/git_cli/**`
- `src/features/review/**`
- 必要的 checkpoint Repository/Migration、fixtures/tests

禁止：操作主工作树 index；使用 `git add .`、`git add -A` 代替明确路径；修改 GAG-011 的删除策略。

## 8. Interface、DTO 与状态

`ReviewService`：`get_worktree_status`、`get_diff`、`create_checkpoint`、`list_checkpoints`、`validate_selection`。

DTO：`FileChange`（path、old_path、kind、binary、size、mode、content/version fingerprint）、`DiffDocument`、`CheckpointSelection`、`CheckpointReceipt`。

Checkpoint 状态：`draft -> validating -> staging -> committing -> committed|failed`。失败后必须恢复或报告 index 状态，不能假装未修改。

SQLite：可新增 append-only Migration 记录 checkpoint ID、task、attempt、commit SHA、selection manifest/hash、created_at；Git commit 是内容真相，数据库用于索引和审计。

## 9. 实施算法与用户流

推荐实施顺序：先完成 porcelain/status 解析与文件 identity 测试，再完成只读 Diff 和 UI，随后实现 selection validation、精确暂存、commit/receipt，最后执行 index 竞态和失败恢复测试。

Review：加载 status snapshot → 选择文件 → 按需加载 Diff → 用户输入提交说明 → 创建 Checkpoint。

创建：持有任务 Git 锁 → 重新读取 HEAD/status → 验证每个选择 fingerprint 未变 → 使用 pathspec-from-file 或等价安全参数精确暂存 → 验证 index 只含选择 manifest → commit → 读取新 SHA/tree → 持久化 receipt → 解锁。

若开始前 index 非空且不是本应用本次事务产生，默认拒绝并给出诊断；不得清空用户 index。

## 10. UI 细节

`UI-REVIEW-001` 左侧文件列表、中央 Diff、右侧/底部 Checkpoint 摘要。选择框必须区分未选择、已选择和 stale；二进制显示元数据而非乱码；超大 Diff 允许分段加载。

创建按钮显示文件数和预计操作，提交中禁用重复点击；成功显示 SHA 和剩余未纳入文件。失败保留选择和提交说明。

## 11. 安全与一致性不变量

- 所有 Git 参数为数组，路径在 `--` 或安全 pathspec 后传递。
- 只暂存当前任务 Worktree 中、用户明确选择且 fingerprint 未变的文件。
- 未选文件、外部绝对路径、`.git` 内部文件不得进入 Checkpoint。
- commit 前后校验 HEAD 和 index；竞态发生即停止。
- 不执行自动 reset/clean 来“修复”意外 index。
- Plan/权限 Guard 必须覆盖暂存和提交写操作。

## 12. 自动化测试

- 新增、修改、删除、rename、mode change、symlink、binary、submodule status 测试。
- 含空格、Unicode、换行/特殊字符文件名的 pathspec 安全测试（按平台能力）。
- 选择后文件改变、HEAD 改变、预存 index、并发 checkpoint 竞态测试。
- 验证未选文件绝不进入 commit。
- 超大/二进制 Diff 按需加载和 UI 状态测试。
- 提交失败后 index 诊断与恢复指引测试。

## 13. 手工验收

1. 修改三个文件，只选两个创建 Checkpoint，commit 仅含两者。
2. 选择后从外部再次修改文件，创建被拒绝并提示 stale。
3. 查看 rename、binary 和超大文件，界面行为明确。
4. 提交失败时选择与说明仍保留，用户修改未丢失。
5. Checkpoint 历史能定位对应任务、attempt 和 commit。

## 14. Definition of Done

- status、Diff、选择、精确暂存、commit 与历史闭环完成。
- 未选改动保护、index 保护和竞态测试通过。
- UI、Git 集成、Migration、Lint、类型检查和构建通过。
- 若由 DeepSeek V4 Pro 实现，Git 安全代码由 Sol 复核。
- 交付报告含选择 manifest 示例、命令数组、测试仓库矩阵和剩余限制。

## 15. 标准任务交付报告

列出 Task ID、模型/reasoning、审查模型、修改文件、Git 命令与锁、Migration、文件类型覆盖、测试/手工证据、安全不变量、已知限制。
