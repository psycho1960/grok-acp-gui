# GAG-009 发布门禁与集成测试计划

| 字段 | 内容 |
| --- | --- |
| 状态 | Draft：发布冻结 |
| 适用任务 | GAG-009 Permissions and Plan Mode |
| 基线 | PR #10，Head `7f164a0da25f51b02e4ba908e718d1c37999aa6e` |
| 目标 | 将复审发现的 7 个 P1 固化为可执行、可审计的发布门禁 |
| 放行 | 全部必测项通过、项目级门禁通过、独立 D5 安全复核通过 |

## 1. 目的、范围和原则

本计划覆盖权限、Plan 审批、ACP 兼容性、持久化并发、秘密脱敏和超时恢复。它以基线 Head 的 7 个 P1 为起点：修复前相应用例应能复现失败；修复后必须稳定通过。

测试以可观察行为为准：数据库状态、发给 ACP 的 JSON-RPC 消息、任务状态和 Renderer 可见内容。测试不得绕过 `TaskRuntime`、`AgentRuntime` 或 DesktopBridge 去验证内部实现细节。

范围来源：`docs/tasks/GAG-009-permissions-and-plan-mode.md`、PR #10 的发布复审结论，以及 `AGENTS.md` 中的 Plan fail-closed、秘密保护、ACP stdout 和 D5 复核要求。本计划不覆盖 GAG-010 Worktree/Git 合并流程；真实 Grok 登录联调受 `AuthorizationRequired` 限制时如实记录，但不能替代 Fake ACP 验收。

## 2. 测试分层与夹具

| 层级 | 职责 | 建议位置 | 环境 |
| --- | --- | --- | --- |
| Rust 单元 | 命令分类、路径校验、参数脱敏 | `task_runtime/**/*.rs` | 临时路径，无真实进程 |
| Rust 持久化集成 | Migration、session 隔离、终态持久化 | `src-tauri/tests/gag_009_permissions_plan.rs` | 独立临时 SQLite |
| Rust 运行时集成 | TaskRuntime 到 AgentRuntime 的完整决策链 | `src-tauri/tests/gag_009_release_gate_integration.rs`（计划新增） | Fake ACP 子进程、临时 DB、临时 workspace |
| Renderer/E2E | 脱敏摘要、可决策/不可决策状态 | `tests/ui/gag-009-permissions-plan.spec.ts`（计划新增） | DesktopBridge mock 或 Fake ACP |
| 项目门禁 | 回归、构建、Windows CI | CI/本地预检 | Windows runner |

运行时集成测试必须启动 `tests/fake-acp-agent/agent.mjs`，而非只 mock Rust 函数。Fake ACP 应捕获 JSON-RPC `id`、方法、会话 ID、原始 option ID 和响应，以证明“未发送”“仅发送一次”以及“原样回传”。每个测试使用独立 task ID、session ID、SQLite 文件和临时工作区；任何外部路径仅作输入，绝不实际删除工作区外文件。

### 2.1 共同断言

* ACP 响应只发送给原 session，保留原始 request ID 和 option ID，不根据标签猜测语义。
* permission 只允许 `requested -> resolved` 或 `requested -> expired`；过期与用户操作竞争时仅有一个终态和一条 ACP 响应。
* plan 与 session、task、version 绑定；ACP 原始 request ID 仅在同一 session 内唯一。
* 秘密测试值统一使用 `GAG009_TEST_SECRET_NEVER_LOG`。数据库摘要、Bridge DTO、Fake ACP 测试日志和 UI 均不得含该值，必须显示统一脱敏标记（当前为 `[redacted]`）。
* 测试失败输出、截图和工件也不得泄露测试秘密或真实凭据。

## 3. 发布阻断测试

### RG-009-P1-01：未批准 Plan 时拒绝写操作

| 项目 | 内容 |
| --- | --- |
| 风险 | Plan mode 在未批准时仍把 `allow_once` 写权限转发给 ACP |
| 前置 | Plan-mode task；`git commit` 等写操作；Fake ACP 等待权限响应 |
| 覆盖状态 | `plan_version=None`，以及最新 Plan 为 `proposed`、`rejected`、`revision_requested`；另测已批准但 version 不匹配 |
| 操作 | 调用 `resolve_permission(allow_once)` |
| 通过条件 | 返回 `PLAN_NOT_APPROVED`；permission 不变为允许；Fake ACP 未收到 allow 型 ResolvePermission；任务不执行写入 |
| 正向对照 | 最新同版本 Plan 为 `approved` 时，可用 ACP 原始 allow option ID 完成一次批准 |

必须为 Fake ACP + TaskRuntime 集成测试；只验证按钮禁用或分类结果不充分。

### RG-009-P1-02：破坏性命令不能借 cwd 逃逸工作区

| 项目 | 内容 |
| --- | --- |
| 风险 | `cwd` 在 workspace 内、`write_paths` 为空时，`rm/rmdir/del/Remove-Item` 可指向 workspace 外 |
| 前置 | workspace 为临时 `repo`；构造 `rm.exe D:/outside/victim.txt`，cwd 为 `repo`，`write_paths=[]`；含 `Remove-Item` 变体 |
| 操作 | 调用操作描述符的范围校验与权限分类 |
| 通过条件 | 校验失败或降级为 `Unknown/Denied`；不提供 allow 选项；不发生文件系统 I/O |
| 正向对照 | 显式声明且规范化后位于 workspace 内的目标按既有规则继续判断 |

至少有 Rust 单元测试；如 Adapter 从 ACP raw input 创建 `Process` 描述符，还须包含一条 adapter 到 TaskRuntime 集成用例。

### RG-009-P1-03：不同 ACP session 可复用原始 request ID

| 项目 | 内容 |
| --- | --- |
| 风险 | 两个 ACP 进程从 `1` 或 `perm-1` 开始时 SQLite 主键冲突，后一任务阻塞 |
| 前置 | 两个不同 session/task 分别提交相同的 permission request ID，亦分别提交相同的 plan request ID |
| 操作 | 保存、读取并分别 resolve 两个 session 的 permission 与 plan |
| 通过条件 | 两个插入均成功；读取/更新严格按 `(session_id, request_id)` 隔离；一方 resolve 不影响另一方 |
| Schema 证据 | `plans`、`permission_decisions` 对 `(session_id, request_id)` 唯一，不再只对原始 ID 全局唯一 |

必须通过实际 Migration 初始化 SQLite；只 mock Repository 的测试无效。

### RG-009-P1-04：`curl -H` 的敏感 Header 不得落库或展示

| 项目 | 内容 |
| --- | --- |
| 风险 | 大写短参数 `-H` 的自定义 Header 值明文持久化、展示 |
| 前置 | Fake ACP 提交 `curl -H "X-Custom-Key: GAG009_TEST_SECRET_NEVER_LOG" https://example.invalid` |
| 操作 | 保存权限请求、读取 DTO、渲染 PermissionSlot |
| 通过条件 | DB 摘要、Bridge DTO、Fake ACP 工件、卡片文本均不含秘密；值显示 `[redacted]`；Header 名可按产品规则保留 |
| 变体 | `-H value`、`-Hvalue`（若协议接受）、`--header value`、大小写混合 Header 名；普通参数不应被过度隐藏 |

必须同时有后端持久化断言和 Renderer 可见文本断言。

### RG-009-P1-05：标准 ACP v1 `toolCall.rawInput` 写请求可批准

| 项目 | 内容 |
| --- | --- |
| 风险 | 不含私有 `operation` 字段的合法 ACP v1 请求被判为 Unknown |
| 前置 | 标准 permission request：`toolCall.rawInput.command="git commit -m test"`，含 allow option，无自定义 `operation` |
| 操作 | Interpreter 解析后进入 TaskRuntime；在已批准 Plan 下执行 allow_once |
| 通过条件 | 归类为写操作并保留原始 allow option；可正常 resolve；Fake ACP 收到原 request ID、原 option ID 的响应 |
| 安全对照 | 含 shell 控制符、重定向、命令替换或不能安全分词的 raw input 必须 fail-closed 为 Unknown/Denied |

输入 JSON 必须只使用 ACP v1 字段，避免测试偷偷依赖项目私有扩展。

### RG-009-P1-06：Fake ACP 的 Plan 可完成批准

| 项目 | 内容 |
| --- | --- |
| 风险 | fixture 发 notification 或遗漏 `kind`，导致 UI 禁用或无法 ResolvePlan |
| 前置 | 启动 `tests/fake-acp-agent/agent.mjs` 的 Plan 场景；Plan 以带 JSON-RPC request ID 的请求发出，并带有具名 `kind` 选项 |
| 操作 | 收到 Plan 后选择批准，等待 Fake ACP 确认 |
| 通过条件 | approve 可操作；Plan 持久化为 `approved`；Fake ACP 收到匹配原 request ID 且含原 approve option ID 的响应；任务退出等待并继续运行 |
| 变体 | reject、revision request 有明确 `kind`；无 ID notification 只能明确显示不可决策/拒绝，绝不可伪造 resolve |

保留子进程级 fixture 测试，防止 Fake ACP 与生产 Interpreter 的 JSON-RPC 契约再次漂移。

### RG-009-P1-07：权限超时自动拒绝并让任务恢复

| 项目 | 内容 |
| --- | --- |
| 风险 | 超过 300 秒后前端仅禁用按钮，后端/ACP 永久等待 |
| 前置 | 创建待决 permission；Fake ACP 等待响应；task 为 `waiting_permission` |
| 操作 | 用注入时钟或 Tokio paused time 推进至 301 秒，不进行用户操作 |
| 通过条件 | permission 为 `expired`；Fake ACP 收到使用原 deny option ID 的拒绝响应；Task 退出 `waiting_permission` 到既定恢复状态；待决映射被清理 |
| 幂等/竞态 | 过期后 resolve 返回不可决策错误且不再发送；用户点击与超时并发时恰好一终态、恰好一响应 |

禁止让测试真实等待 301 秒。若运行时不能注入或暂停时间，这一可测试性缺口属于修复内容。

## 4. 交叉回归

| ID | 场景 | 通过条件 |
| --- | --- | --- |
| RG-009-X-01 | 未批准 Plan 下的只读命令 | 保持既有允许流程，不误拦截 |
| RG-009-X-02 | 合法 workspace 内写路径 | 正常出现权限请求，但仍受 Plan 状态约束 |
| RG-009-X-03 | 已 resolved/expired 请求的重复点击 | 无第二条 ACP 响应、无状态回退 |
| RG-009-X-04 | 两个并行 task 的 Plan 与 permission | 事件、DB 读取和 ACP 响应全按 session/task 隔离 |
| RG-009-X-05 | 错误、日志、浏览器控制台 | 不含 token、API key、环境变量全值或测试秘密 |

## 5. 实现映射与执行

下表是计划测试落点；文件未创建前不得报告为已执行。

| 用例 | 测试落点 | 类型 | 基线预期 |
| --- | --- | --- | --- |
| P1-01 | `src-tauri/tests/gag_009_release_gate_integration.rs` | Fake ACP + TaskRuntime | 失败 |
| P1-02 | `src-tauri/tests/gag_009_permissions_plan.rs` | 单元/持久化 | 失败 |
| P1-03 | `src-tauri/tests/gag_009_permissions_plan.rs` | SQLite Migration 集成 | 失败 |
| P1-04 | 后端测试 + `tests/ui/gag-009-permissions-plan.spec.ts` | 持久化 + UI | 失败 |
| P1-05 | `src-tauri/tests/gag_009_release_gate_integration.rs` | ACP v1 契约 | 失败 |
| P1-06 | `src-tauri/tests/gag_009_release_gate_integration.rs` | Fake ACP 子进程 | 失败 |
| P1-07 | `src-tauri/tests/gag_009_release_gate_integration.rs` | 可控时钟集成 | 失败 |

执行顺序：先 P1-02/P1-04 的安全单元测试，再 P1-03 Migration 隔离，最后 P1-01/P1-05/P1-06/P1-07 的完整 ACP 链路；每项修复先让对应测试由红转绿，再跑交叉回归。

计划中的针对性命令（对应测试文件创建后执行）：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test gag_009_permissions_plan
cargo test --manifest-path src-tauri/Cargo.toml --test gag_009_release_gate_integration
npm run test -- --runInBand
```

Windows 若有调试应用锁定默认 Cargo target，可为测试设置独立的临时 `CARGO_TARGET_DIR`；不得复用或删除用户的构建目录。

## 6. 发布门禁与复审证据

必须全部满足：

1. P1-01 至 P1-07 与交叉回归全部通过，并记录测试名、命令、commit 和结果。
2. 每个新增回归测试在修复前验证过对应失败模式，或保留可审计的失败证据。
3. 最新 Head 通过 `npm run typecheck`、`npm run lint`、`npm run test`、`cargo fmt --check --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`cargo test --manifest-path src-tauri/Cargo.toml`、`npm run build`、`npm run tauri build`。
4. Windows CI 必须在包含修复与测试的最新 Head 绿色；旧 Head 的 CI 不能替代。
5. 不同旗舰模型完成 D5 独立安全复核，至少覆盖 Plan fail-closed、路径范围、并发主键、脱敏、ACP v1 与超时竞态。
6. PR #10 在证据齐全前保持 Draft；不得将未验证的本地修改称为已修复。

以下不构成放行证据：只通过 UI 快照、只通过单元测试、旧 Head 的绿色 CI、受限真实 Grok 登录后的主观判断，或包含真实凭据的日志/截图。

## 7. 复审记录模板

```markdown
## GAG-009 发布门禁复审记录

- 被测 commit：
- 环境：Windows / Node / Rust / SQLite：
- Fake ACP fixture commit：
- P1-01 至 P1-07：命令、结果、证据路径：
- 交叉回归：
- 项目级预检：
- Windows CI：
- D5 独立复核（模型、范围、结论）：
- 真实 Grok ACP 联调（可选；结果或受阻原因）：
- 发布结论：通过 / 不通过
```

## 8. 当前结论

基线 `7f164a0` 上 P1-01 至 P1-07 均为发布阻断项；已有 Windows CI 通过不构成放行。本文是后续修复、精确集成测试和独立复审共同遵循的验收合同；只有在最新 PR Head 上实际运行后，状态才可更新为通过。
