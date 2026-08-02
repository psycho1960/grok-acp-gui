# Grok ACP GUI 产品需求文档

## 1. 文档目的

本文定义 Grok ACP GUI v1 的意图、边界、行为和可验收结果。目标读者是产品评审者、UI/UX 设计者、开发者、测试者和后续编码 Agent。本文是“应该做什么”的规范；布局细节见 `02-UI-UX-DESIGN.md`，实现方法见 `03-TECHNICAL-DESIGN.md`，实施顺序见 `04-AI-DEVELOPMENT-ROADMAP.md`。

## 2. 产品概述

Grok ACP GUI 是 Windows 优先的本地桌面应用，以结构化 ACP 连接 Grok Build，为不希望长期停留在终端中的开发者提供可观察、可审批、可恢复的编码工作流。产品不重写 Grok 的 Agent 循环，而是在其上提供项目、任务、图片、权限、Diff 和 Worktree 集成体验。

### 2.1 目标用户

- 主要用户：个人开发者及 2–10 人小团队。
- 使用环境：Windows 10/11、本地 Git 仓库、已安装或愿意安装 Grok Build。
- 核心诉求：看清 Agent 正在做什么；在危险操作前审批；并行任务互不污染；完成后容易审查与合并。

### 2.2 产品目标

1. 用户无需解析终端输出即可运行 Grok Build 编码任务。
2. 用户可在一个窗口内完成任务创建、对话、审批、Diff、检查点、合并和清理。
3. 写入型并行任务默认使用隔离 Worktree，避免互相覆盖。
4. 图片输入与结果预览在会话恢复后仍然可用。
5. 所有破坏性 Worktree 操作都有可验证的安全前置条件和恢复路径。

### 2.3 非目标

- 不做完整 IDE、LSP、调试器或交互式终端。
- 不支持 Web、移动端、macOS 或 Linux 首发。
- 不支持其他 Agent、通用 Agent 商店或插件市场管理。
- 不自动 Push、创建 PR、删除远程分支或执行远程写入。
- 不提供语音输入和专用图片/视频生成工作台。
- 不保存 xAI/Grok Token 或 API Key。

## 3. 成功标准

- 首次用户在 5 分钟内完成 Grok 检测/登录、打开项目并发送第一条任务。
- 用户能同时运行至少 3 个隔离任务，事件、权限和文件变化不串线。
- 应用崩溃或重启后，已持久化任务可恢复；运行中状态不会误显示为仍在运行。
- 原工作区在任务合并冲突时保持干净。
- 未合并的受管 Worktree 不会在缺少恢复包时被强制删除。
- 所有 D5 流程具有自动化负面测试和独立旗舰模型审查。

## 4. 核心用户旅程

### 4.1 首次启动

1. 应用探测 Grok CLI、Git、版本和登录状态。
2. Grok 缺失时显示官方安装引导；版本低于最低版本时提供更新说明。
3. 未登录时启动 `grok login`，应用等待并重新探测，不读取或保存凭据。
4. 探测成功后进入项目选择页。

### 4.2 创建任务

1. 用户选择本地目录并确认信任。
2. 应用识别 Git 仓库、当前分支、脏状态和现有 Worktree。
3. 用户新建任务、输入目标、添加图片并选择模式。
4. Agent/Plan 默认隔离 Worktree；Ask 默认当前目录；用户可以显式切换。
5. 隔离任务创建受管分支、Worktree 和 ACP session 后进入对话。

### 4.3 执行与审批

1. 用户发送文本、图片或文件引用。
2. 时间线流式展示回复、思考摘要、计划和工具调用。
3. 权限请求阻塞对应工具，界面呈现 ACP 提供的原始选项。
4. Plan 未批准时，客户端额外阻止写入和非只读命令。
5. 用户可取消当前 Turn；取消不得丢失已完成事件和文件状态。

### 4.4 审查与合并

1. 任务完成后进入待审查状态。
2. 用户查看所有 Changes 和逐文件完整 Diff。
3. 用户选择文件创建检查点；未选文件继续留在 Worktree。
4. 应用在临时 integration Worktree 中试做 Squash 合并。
5. 无冲突且目标 HEAD 未变化时，把集成提交 fast-forward 到目标分支。
6. 合并后用户可清理受管 Worktree；未选变更会阻止普通清理。

### 4.5 失败与恢复

- ACP 进程异常退出：任务标记 interrupted，清除悬挂权限并允许恢复。
- Worktree 创建失败：保留任务草稿、提示失败阶段并允许重试。
- 合并冲突：只保留在临时 integration Worktree，原工作区不变。
- 强制丢弃：先创建并验证恢复包，随后才允许删除。
- 图片源丢失：已缓存附件继续可用；缓存也丢失时显示不可恢复状态而非空白。

## 5. 功能需求

### 5.1 Runtime

- **FR-RUNTIME-001**：按“用户设置路径 → `%USERPROFILE%\.grok\bin\grok.exe` → PATH”探测 Grok。
- **FR-RUNTIME-002**：显示 installed、version、authenticated、ready 和 actionable error 状态。
- **FR-RUNTIME-003**：通过 `grok --no-auto-update agent stdio` 建立 ACP，并执行协议版本与 capability negotiation。
- **FR-RUNTIME-004**：支持调用 `grok login`，不持久化凭据。
- **FR-RUNTIME-005**：Grok 版本低于 0.2.118 时阻止会话启动并显示升级动作。

### 5.2 Project

- **FR-PROJECT-001**：选择、验证、信任和记住本地项目目录。
- **FR-PROJECT-002**：展示仓库根目录、分支、HEAD、脏状态及 Worktree 数量。
- **FR-PROJECT-003**：非 Git 目录允许 Ask/普通任务，但禁用 Worktree、检查点和集成。
- **FR-PROJECT-004**：项目路径变更或不可访问时显示恢复动作，不静默移除历史。

### 5.3 Task and Session

- **FR-TASK-001**：创建任务时保存标题、初始 Prompt、模式、模型、工作区策略和附件。
- **FR-TASK-002**：Agent/Plan 默认隔离，Ask 默认当前目录，用户可覆盖。
- **FR-TASK-003**：任务状态包括 draft、preparing、running、waiting_permission、idle、interrupted、failed、ready_for_review、integrating、conflicted、merged、archived。
- **FR-TASK-004**：并行运行任务不设硬上限；第 4 个同时运行的 Turn 起显示资源警告但不阻止。
- **FR-SESSION-001**：支持创建、恢复、关闭和取消会话 Turn。
- **FR-SESSION-002**：流式事件按 taskId、sessionId、seq 去重和排序。
- **FR-SESSION-003**：模型、模式、reasoning 和 Slash Commands 从 ACP 动态获取。
- **FR-SESSION-004**：空闲任务关闭 ACP 进程，后续通过 session resume 恢复。
- **FR-SESSION-005**：未知 ACP 事件降级为通用事件卡，不能导致会话崩溃。

### 5.4 Permission and Plan

- **FR-PERMISSION-001**：显示请求标题、工具类别、影响路径/命令摘要及 ACP 原始选项。
- **FR-PERMISSION-002**：决定回传原始 option ID；不得根据标签猜测“永久允许”。
- **FR-PERMISSION-003**：进程退出、取消或超时时清除/拒绝悬挂请求并记录结果。
- **FR-PLAN-001**：Plan 阶段客户端 fail-closed 阻止写文件和非只读命令。
- **FR-PLAN-002**：支持批准、继续规划和取消；实际选项以 capability 为准。

### 5.5 Image and Artifact

- **FR-IMAGE-001**：支持粘贴、拖放和文件选择 PNG/JPEG/GIF/WebP。
- **FR-IMAGE-002**：单张最大 20 MiB；格式和大小不合法时发送前阻止并给出原因。
- **FR-IMAGE-003**：附件复制到受管缓存，以哈希去重并生成缩略图。
- **FR-IMAGE-004**：对话内预览图片结果，支持全图、另存为和打开所在位置。
- **FR-IMAGE-005**：会话恢复时图片 chip、来源关系和缓存状态可恢复。

### 5.6 Worktree

- **FR-WORKTREE-001**：使用 Git 结构化输出发现全部 Worktree，区分应用受管、外部和已接管。
- **FR-WORKTREE-002**：受管分支命名 `grok/<slug>-<shortId>`，路径位于 `%LOCALAPPDATA%\GrokAcpGui\worktrees\<repoHash>\<taskId>`。
- **FR-WORKTREE-003**：外部 Worktree 默认只能查看和打开，接管需明确确认。
- **FR-WORKTREE-004**：同一仓库的集成和清理使用互斥锁，其他 Agent Turn 可继续。
- **FR-WORKTREE-005**：Worktree session resume 时必须验证 cwd 与绑定仍一致。

### 5.7 Review, Integration and Recovery

- **FR-REVIEW-001**：展示文件状态、增删统计、二进制/过大降级和完整文本 Diff。
- **FR-REVIEW-002**：用户按文件选择创建检查点；v1 不做 hunk staging。
- **FR-REVIEW-003**：检查点使用用户确认的提交信息；Git identity 缺失时给出可执行修复。
- **FR-REVIEW-004**：Squash 在临时 integration Worktree 完成，目标分支只接收通过验证的集成提交。
- **FR-REVIEW-005**：目标工作区不干净、分支不匹配或 HEAD 变化时阻止集成。
- **FR-RECOVERY-001**：强制清理前生成 branch bundle、tracked binary patch、未忽略 untracked 文件和元数据。
- **FR-RECOVERY-002**：恢复包默认保留 7 天，可查看、恢复或手动删除。
- **FR-RECOVERY-003**：恢复包创建/验证失败时不得删除 Worktree。

## 6. 非功能需求

- **NFR-SECURITY-001**：Renderer 无任意 Shell、Git、文件系统、SQLite 或 ACP 直接访问权。
- **NFR-SECURITY-002**：路径规范化后必须位于允许的项目、缓存、临时 integration 或受管 Worktree 根目录。
- **NFR-SECURITY-003**：日志脱敏；应用不保存凭据。
- **NFR-SECURITY-004**：所有 destructive action 显示精确目标、不可逆影响和恢复状态。
- **NFR-PERFORMANCE-001**：长会话、Diff 和图片列表采用虚拟化或按需加载。
- **NFR-PERFORMANCE-002**：流式事件批量刷新，正常 60 TPS 输出不造成明显 UI 冻结。
- **NFR-RELIABILITY-001**：应用关闭后不得残留受管 Grok 子进程。
- **NFR-RELIABILITY-002**：SQLite 写入采用事务，Migration 失败不启动主界面。
- **NFR-ACCESSIBILITY-001**：状态不能只靠颜色表达；关键操作支持键盘和清晰焦点。
- **NFR-ACCESSIBILITY-002**：文本与背景达到 WCAG AA 对比度。
- **NFR-PRIVACY-001**：默认无遥测，无自动上传项目、图片或日志。

## 7. 配置与秘密

| 名称 | 使用方 | 来源 | 是否秘密 | 持久化策略 |
|---|---|---|---|---|
| Grok executable path | Runtime | 自动探测/用户设置 | 否 | 本地设置 |
| XAI_API_KEY | Grok 子进程 | 父进程环境 | 是 | 应用不保存 |
| Grok login token | Grok CLI | Grok 自有存储 | 是 | 应用不读取 |
| Managed worktree root | Workspace | 应用默认/设置 | 否 | 本地设置 |
| Log level | Diagnostics | 应用设置 | 否 | 本地设置 |

## 8. 验收场景

1. 无 Grok、未登录、版本过低均出现不同且可执行的引导。
2. 路径含空格和中文的 Git 项目可创建隔离任务并恢复。
3. 三个并行任务的消息、权限和变更互不串线；第四个出现警告但可运行。
4. Plan 未批准时模拟写入和危险命令均被客户端拒绝。
5. PNG/JPEG/GIF/WebP 的粘贴、拖放、超限和恢复路径通过。
6. 用户选择部分文件创建检查点，未选变化不进入集成提交。
7. 目标分支前进或产生冲突时原工作区保持不变。
8. 脏 Worktree 强制清理失败于恢复包生成时，不执行删除。
9. 强制关闭应用后无子进程残留，任务显示 interrupted 并可恢复。

## 9. 需求追踪矩阵

下表中的测试族由对应任务实现，GAG-015 汇总执行并形成最终证据。范围记法包含范围内的每一个 Requirement ID。

| Requirement | UI | 技术 Module | 实施任务 | 测试/证据族 |
|---|---|---|---|---|
| FR-RUNTIME-001～005 | UI-ONBOARD-001、UI-SETTINGS-001、UI-ERROR-001 | MOD-AGENT-RUNTIME、MOD-PERSISTENCE | GAG-003～005、GAG-016 | TST-RUNTIME：探测/握手/版本/退出/Fake ACP |
| FR-PROJECT-001～004 | UI-PROJECT-001、UI-TASK-001、UI-WORKTREE-001 | MOD-TASK-RUNTIME、MOD-WORKSPACE、MOD-PERSISTENCE | GAG-004、GAG-007、GAG-011 | TST-PROJECT：路径/持久化/重启/受管状态 |
| FR-TASK-001～004 | UI-TASK-001、UI-TASK-002、UI-CONV-001 | MOD-TASK-RUNTIME、MOD-PERSISTENCE | GAG-004、GAG-006～007 | TST-TASK：创建/并发/状态/取消/恢复 |
| FR-SESSION-001～005 | UI-CONV-001、UI-TASK-002、UI-RECOVERY-001 | MOD-AGENT-RUNTIME、MOD-TASK-RUNTIME、MOD-PERSISTENCE | GAG-005～006、GAG-008 | TST-SESSION：snapshot/delta/乱序/中断/重连 |
| FR-PERMISSION-001～003 | UI-PERM-001、UI-CONV-001 | MOD-TASK-RUNTIME、MOD-AGENT-RUNTIME | GAG-009 | TST-PERMISSION：过期/复用/拒绝/绕过/fail-closed |
| FR-PLAN-001～002 | UI-PLAN-001、UI-CONV-001 | MOD-TASK-RUNTIME、MOD-AGENT-RUNTIME | GAG-009 | TST-PLAN：版本绑定/写门禁/修改/拒绝 |
| FR-IMAGE-001～005 | UI-ARTIFACT-001、UI-CONV-001 | MOD-ARTIFACTS、MOD-PERSISTENCE | GAG-010 | TST-ARTIFACT：导入/MIME/限额/预览/路径安全 |
| FR-WORKTREE-001～005 | UI-WORKTREE-001、UI-RECOVERY-001 | MOD-WORKSPACE、MOD-PERSISTENCE | GAG-011、GAG-014 | TST-WORKTREE：创建/对账/路径逃逸/恢复包/清理 |
| FR-REVIEW-001～005 | UI-REVIEW-001、UI-RECOVERY-001 | MOD-WORKSPACE、MOD-PERSISTENCE | GAG-012～014 | TST-REVIEW：Diff/精确暂存/Checkpoint/CAS/冲突 |
| FR-RECOVERY-001～003 | UI-RECOVERY-001、UI-TASK-002 | MOD-TASK-RUNTIME、MOD-WORKSPACE、MOD-ARTIFACTS、MOD-PERSISTENCE | GAG-006、GAG-014 | TST-RECOVERY：扫描/故障注入/bundle/重复执行 |
| NFR-SECURITY-001～004 | 全部，重点 UI-PERM-001、UI-PLAN-001、UI-RECOVERY-001 | 全部 Module 与 Adapter | GAG-003、GAG-009～015 | TST-SECURITY：信任边界/路径/权限/Git/秘密/XSS |
| NFR-PERFORMANCE-001～002 | UI-TASK-001、UI-CONV-001、UI-ARTIFACT-001、UI-REVIEW-001 | MOD-TASK-RUNTIME、MOD-ARTIFACTS | GAG-007～008、GAG-010、GAG-015 | TST-PERFORMANCE：500 tasks/10k events/delta burst/大文件 |
| NFR-RELIABILITY-001～002 | UI-ERROR-001、UI-RECOVERY-001 | MOD-AGENT-RUNTIME、MOD-TASK-RUNTIME、MOD-PERSISTENCE | GAG-004～006、GAG-014～016 | TST-RELIABILITY：进程回收/事务/Migration/崩溃恢复 |
| NFR-ACCESSIBILITY-001～002 | 全部 UI | Renderer App Shell 与 Features | GAG-002、GAG-007～010、GAG-015 | TST-A11Y：键盘/焦点/名称/对比度/reduced-motion |
| NFR-PRIVACY-001 | UI-SETTINGS-001、UI-ERROR-001 | 全部 Adapter、日志与打包 | GAG-001、GAG-005、GAG-010、GAG-015～016 | TST-PRIVACY：遥测/网络/日志/数据库/构建产物扫描 |
