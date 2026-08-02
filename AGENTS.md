# Grok ACP GUI 项目级开发与 Git 规范

本文件是仓库级编码 Agent 规范。所有人类开发者、自动化工具和 AI Agent 在修改仓库前必须阅读并遵守。已分配的 `docs/tasks/GAG-*.md` 即视为实施授权；只有规格冲突、范围扩张、破坏性操作无法满足安全前置条件或外部权限缺失时才暂停并请求确认。

## 1. 规范源与优先级

发生冲突时按以下顺序处理：

1. 用户在当前任务中的明确指令。
2. 本文件。
3. 当前 `GAG-*.md` 任务说明书。
4. `docs/01-PRD.md`、`docs/02-UI-UX-DESIGN.md`、`docs/03-TECHNICAL-DESIGN.md`、`docs/04-AI-DEVELOPMENT-ROADMAP.md`。
5. 已接受的 `docs/adr/` 决策记录。

任务说明书统一位于 `docs/tasks/`；实施前必须打开当前 `GAG-*.md` 并确认 Roadmap 中的依赖和模型门禁。

不得自行解决文档之间的实质冲突。发现冲突时说明涉及的 ID、影响和可选方案，并停止冲突范围内的修改。

## 2. 任务、分支与提交

- 每个任务使用独立分支，禁止直接在 `main` 开发。
- 分支格式：`feat/GAG-001-project-bootstrap`、`fix/GAG-014-worktree-cleanup`、`docs/GAG-003-desktop-bridge-contracts`。
- 一个分支只包含当前任务要求的代码、文档、测试和 Migration。禁止顺手实现后续任务、跨模块重构或全局格式化。
- Conventional Commit 格式：`feat(GAG-001): bootstrap desktop application`。
- 允许类型：`feat`、`fix`、`docs`、`test`、`chore`、`refactor`、`build`、`ci`。
- 提交保持原子性；格式化和业务修改不得混在同一提交。
- 同步主分支只能使用 `git fetch origin` 后 `git rebase origin/main`，禁止以 merge commit 同步。
- `main` 只接受通过检查的 PR，默认且唯一合并方式为 Squash Merge。

## 3. 开发前规格门禁

开始修改前必须检查并在工作记录中确认：

- 功能目标：已理解或存在冲突。
- 用户场景与页面流转：已理解或不涉及 UI。
- 页面、组件和全部状态：已定义或存在缺口。
- 业务规则与状态机：已定义或存在冲突。
- DesktopBridge Interface、ACP 消息或事件：已定义或不涉及。
- 数据与 SQLite Migration：已定义或不涉及。
- Git、Worktree、文件和进程副作用：已定义或不涉及。
- 异常、恢复、安全不变量：已覆盖或存在缺口。
- 验收标准：可执行或存在不可执行项。
- 自动化测试和手工验收：已明确。
- 本期不实现：已确认。
- 修改范围：与任务说明书一致。

规格完整且任务已分配时直接实施，不重复请求确认。

## 4. 目录与模块纪律

### Renderer

- `src/features/` 只负责界面行为和用户交互。
- Renderer 只能通过 `src/bridge/` 的 DesktopBridge Interface 调用后端。
- 禁止在 Vue、Pinia 或共享 UI 中直接调用 Tauri command、ACP、Git、SQLite、文件系统或任意 Shell。
- 共享视觉只能引用 `src/shared/theme/` 的 Catppuccin Mocha Design Token，禁止散落硬编码品牌色。

### Rust 后端

- `src-tauri/src/bridge/` 只做反序列化、验证、调用模块和把结果映射为 DTO/事件，不放业务规则。
- 业务实现集中在五个深模块：`agent_runtime`、`task_runtime`、`workspace`、`artifacts`、`persistence`。
- 外部 I/O 位于 Adapter：`grok_acp`、`git_cli`、`filesystem`、`sqlite`。
- 一个 Module 只有一个供调用方理解的 Interface。不要为单个命令创建浅层 Module。
- 只有生产实现与测试替身都真实存在时才定义 seam；禁止预设未来可能用到的 Adapter。
- 测试通过调用方使用的 Interface 验证可观察行为，不越过 Interface 测实现细节。

## 5. Grok、ACP 与进程安全

- 禁止解析 Grok TUI 的视觉文本输出。
- Grok 集成必须使用 ACP 或明确记录的结构化 CLI 输出。
- `grok agent stdio` 的 stdout 只能承载 JSON-RPC；日志写入 stderr 或独立脱敏日志。
- 权限按钮必须原样回传 ACP 提供的 option ID，不根据标签猜测权限语义。
- Plan 未批准时必须 fail-closed 阻止文件写入和非只读命令。
- Renderer 不得获得任意 Shell Interface。
- 应用关闭、任务取消和进程崩溃必须清理或可恢复地处理子进程树及悬挂请求。
- 日志不得包含 Token、API Key、环境变量全值、用户图片内容或未脱敏命令环境。

## 6. Git 与 Worktree 安全

- Git 命令必须通过可执行文件和参数数组调用，禁止拼接 Shell 命令字符串。
- 所有路径必须规范化并验证仓库、工作区和受管 Worktree 根目录关系。
- 普通删除只允许作用于已验证的应用受管 Worktree。
- 外部 Worktree 默认只读管理；用户明确接管后才能执行合并或清理。
- 未合并或脏 Worktree 强制清理前必须成功生成恢复包并验证其非空。
- 合并前必须验证目标工作区干净、目标分支正确、记录的 HEAD 未变化。
- 冲突试合并只允许发生在临时 integration Worktree，不能污染原工作区。
- 禁止自动 Push、创建 PR、删除远程分支或执行强制远程写入。

## 7. 数据、附件与秘密

- SQLite Migration 一经合并不得修改；修正必须新增 Migration 并说明升级和回滚策略。
- ACP session 是对话事实来源；SQLite 保存项目、任务、会话绑定、Worktree、附件和恢复记录。
- 图片只接受 PRD 允许的格式和大小；缓存使用受管目录、随机/哈希文件名和路径验证。
- 禁止把真实密钥、个人数据、缓存、构建产物或本地环境配置提交到 Git。
- 应用不保存 Grok Token 或 API Key；复用 Grok CLI 登录或进程继承的环境变量。

## 8. 单任务执行准则

### 执行中

- 只修改任务说明书列出的允许范围。确需扩大范围时先说明原因、具体文件和影响。
- 不删除已有功能，不修改无关代码，不进行全局格式化。
- 不擅自改变 DesktopBridge、状态名、数据库字段、Design Token、认证方式、合并策略或安全不变量。
- 新事件和 DTO 必须同步 Rust、TypeScript、测试和技术文档。
- 生产代码不得遗留 mock、调试身份、明文密钥、敏感 console 日志或无责任人的 TODO。

### 完成后

- 运行任务说明书要求的类型检查、Lint、测试和构建。
- 输出实际新增、修改和删除文件。
- 输出 Interface、事件、Migration 和配置变化。
- 输出测试命令、结果和未执行原因。
- 输出风险、兼容性、手工验收和未完成事项。
- 不得只回复“已完成”。

## 9. 模型使用与升级

- 每个任务使用任务说明书指定的首选模型；使用备选模型或降级模型时在交付报告记录原因。
- Flash/Luna 连续两次无法通过同一验收项时升级 Terra。
- Terra 或 Grok 4.5 遇到跨层状态不一致、安全或竞态时升级 Sol。
- DeepSeek V4 Pro 完成安全关键 Git/权限实现后，由 Sol 独立复核。
- D5 任务合并前必须由不同旗舰模型做一次独立审查。
- Luna 不得单独处理 ACP、Plan、安全权限、Git 合并、Worktree 删除、Migration 或并发恢复。

## 10. PR 前预检

至少运行：

```powershell
npm run typecheck
npm run lint
npm run test
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
npm run tauri build
```

任务说明书要求的 ACP、Git 或 E2E 测试必须额外运行。无法执行的检查要记录环境原因，不能虚构通过。

## 11. 统一交付模板

### 一、任务完成情况

- 已完成：
- 未完成：
- 与计划差异：
- 实际使用模型及升级/降级：

### 二、修改文件

- 新增：
- 修改：
- 删除：

### 三、Interface 与数据变化

- DesktopBridge/事件/DTO：
- SQLite Migration：
- 状态机：
- 回滚方式：

### 四、测试结果

- TypeScript 类型检查：
- 前端 Lint/测试：
- Rust fmt/clippy/test：
- ACP 契约测试：
- Git 集成测试：
- Windows E2E：
- Tauri Build：
- 手工验收：

### 五、风险与兼容性

- 风险：
- 对已有功能的影响：
- 性能与安全注意事项：

### 六、后续事项

- 明确未完成项：
- 建议下一个任务：

