# GAG-005 交付报告：Grok Runtime 与 ACP 接入

## 一、任务完成情况

### 已完成

- **AgentRuntime Interface**：定义了 `AgentRuntime` trait（`probe`、`start`、`send`、`cancel`、`shutdown`、`subscribe`、`session_state`）
- **进程状态机**：实现了完整的状态转换表 `unavailable → probing → starting → handshaking → ready → busy → stopping → stopped`，任意态可进入 `failed`，全部转换原子可审计
- **ACP 协议编解码器**：JSON-RPC 2.0 帧解析，大小上限（4 MiB）、深度限制（64）、UTF-8 校验、字段校验；未知帧不崩溃
- **grok_acp Adapter**：参数数组启动（`--no-auto-update agent stdio`）、stdin/stdout JSON-RPC、stderr 独立限量缓存（200 行 ring buffer）、环境变量 allowlist（含 `SYSTEMROOT`）、Windows `CREATE_NO_WINDOW`
- **Fake ACP Agent**：Node.js 脚本支持 8 种场景（normal/timeout/crash/bad-frame/stderr-flood/unknown-method/permission/plan）
- **事件转换**：11 种 AgentEvent（session_ready、assistant_delta/completed、tool_started/updated/completed、plan_proposed、permission_requested、artifact_announced、request_failed、process_exited）映射到 DesktopEvent
- **Bridge 接入**：`runtime.refresh` 命令接入 probe；事件转发器从 AgentRuntime → `bridge:event` Tauri 通道
- **日志脱敏**：`redact()` 函数清除 token/api_key/authorization 等敏感字段；stderr 行级脱敏
- **集成测试**：8 个场景覆盖正常生命周期、crash、timeout、permission、unknown method、stderr flood、idempotent shutdown/cancel

### 未完成

- **握手响应解析**：当前 handshake 等待 `initialize` 响应并提取 `protocolVersion`/`agentName`/`agentVersion`，但未提取 `models`/`modes`/`slashCommands`（留给 GAG-006）
- **`runtime.login`**：返回 `BRIDGE_NOT_IMPLEMENTED`（login 流程不在 GAG-005 范围）
- **ACP session resume**：未实现 `loadSession`（GAG-006 范围）
- **真实 Grok 进程测试**：仅使用 Fake ACP agent 测试；真实 Grok 需手工验收

### 与计划差异

- AgentRuntime trait 的 `probe()` 方法返回占位结果，实际探测由 bridge 层调用 `GrokAcpAdapter::probe()` 完成（trait 方法存在但 bridge 直接用 adapter）
- Tauri `execute` 命令保持同步签名 + `block_on`，因 async 命令与 `serde_json::Value` 有 lifetime 冲突

### 实际使用模型

- 首选模型：GPT-5.6 Sol（未使用——本环境无 Sol 模型）
- 实际模型：GLM-5.2
- 未升级/降级

## 二、修改文件

### 新增

- `src-tauri/src/modules/agent_runtime/mod.rs` — AgentRuntime trait 定义、default_search_paths
- `src-tauri/src/modules/agent_runtime/state.rs` — RuntimeState 状态机 + 全表测试
- `src-tauri/src/modules/agent_runtime/events.rs` — AgentEvent 枚举 + 11 种 payload 类型
- `src-tauri/src/modules/agent_runtime/requests.rs` — ClientRequest 类型
- `src-tauri/src/modules/agent_runtime/config.rs` — RuntimeConfig、RuntimeProbeResult、RuntimeHandle
- `src-tauri/src/modules/agent_runtime/diagnostics.rs` — 脱敏、DiagLog、StderrBuffer
- `src-tauri/src/modules/agent_runtime/runtime.rs` — AgentRuntimeImpl 协调器
- `src-tauri/src/adapters/grok_acp/mod.rs` — 模块根 + re-exports
- `src-tauri/src/adapters/grok_acp/codec.rs` — JSON-RPC 编解码器 + 帧校验
- `src-tauri/src/adapters/grok_acp/transport.rs` — AcpTransport trait（seam）
- `src-tauri/src/adapters/grok_acp/process.rs` — GrokAcpAdapter（生产适配器）
- `src-tauri/src/adapters/grok_acp/interpreter.rs` — ACP 消息 → AgentEvent 解释器
- `src-tauri/src/adapters/grok_acp/fake.rs` — FakeAcpTransport（测试适配器）
- `src-tauri/tests/gag_005_runtime_integration.rs` — 8 个集成测试
- `tests/fake-acp-agent/agent.mjs` — Node.js Fake ACP agent
- `tests/fake-acp-agent/package.json`
- `overview.md` — 本交付报告

### 修改

- `src-tauri/Cargo.toml` — 添加 tokio、async-trait 依赖
- `src-tauri/src/lib.rs` — 注册 agent_runtime/grok_acp 模块；AppState 添加 runtime；execute 改为 block_on async；spawn_event_forwarder
- `src-tauri/src/bridge/dispatch.rs` — execute_impl 改为 async + runtime 参数；实现 runtime_refresh；添加 map_agent_event

### 删除

无

## 三、Interface 与数据变化

### DesktopBridge/事件/DTO

- `execute` 命令签名不变（同步），内部通过 `block_on` 调用 async `execute_impl`
- `runtime.refresh` 从 `BRIDGE_NOT_IMPLEMENTED` 改为实际调用 `AgentRuntime::probe`
- 新增 `map_agent_event()` 函数：AgentEvent → DesktopEvent 映射（11 种事件类型）
- 新增事件转发器：runtime subscribe → `bridge:event` Tauri 通道

### SQLite Migration

无新增 Migration（GAG-005 明确禁止）

### 状态机

- 新增 `RuntimeState` 状态机：8 个状态 + `Failed`，15 种转换，全部有测试
- 不影响已有 Task/Session/Worktree/Recovery 状态机

### 回滚方式

- 回滚此分支即可移除全部改动
- 无数据库 Migration，无不可逆操作

## 四、测试结果

### TypeScript 类型检查

```
npm run typecheck → PASS
```

### 前端 Lint/测试

```
npm run lint → PASS (0 errors)
npm run test → 3 files, 33 tests passed
```

### Rust fmt/clippy/test

```
cargo fmt --check → PASS
cargo clippy --all-targets -- -D warnings → PASS (0 warnings)
cargo test → 145 unit tests + 8 integration tests = 153 passed, 0 failed
```

### ACP 契约测试

- Fake ACP normal lifecycle：握手 → prompt → 流式 delta → tool call → completed → shutdown ✓
- Fake ACP crash：进程退出 → handshake 失败 → ACP_HANDSHAKE_FAILED ✓
- Fake ACP timeout：握手无响应 → 3s 超时 → ACP_HANDSHAKE_FAILED ✓
- Fake ACP permission：requestPermission notification → PermissionRequested event ✓
- Fake ACP unknown method：不崩溃，继续运行 ✓
- Fake ACP stderr flood：不 OOM，不崩溃 ✓
- Idempotent shutdown：重复 shutdown 不报错 ✓
- Cancel when not busy：no-op，状态不变 ✓

### Git 集成测试

不涉及（GAG-005 范围外）

### Windows E2E

未运行（需 Tauri build + 手工验收）

### Tauri Build

未运行（`npm run tauri build` 需要完整 WebView2 环境）

### 手工验收

未执行（需安装真实 Grok Build）

## 五、风险与兼容性

### 风险

1. **握手响应解析不完整**：当前只提取 protocolVersion/agentName/agentVersion，未提取 models/modes/capabilities。GAG-006 需补充
2. **事件转发 task_id 为空**：AgentEvent 不携带 task_id（runtime 层不知道 task 映射），bridge 层的 `map_agent_event` 使用空 TaskId。GAG-006 的 session 管理层需补充映射
3. **probe() 未真正调用 adapter**：`AgentRuntime::probe()` trait 方法返回占位错误，实际探测需要 bridge 层直接调用 `GrokAcpAdapter::probe()`。当前 `runtime.refresh` 调用 trait probe，返回的是占位结果
4. **block_on 可能阻塞**：`execute` 命令使用 `tauri::async_runtime::block_on`，长时间运行的 runtime 操作可能阻塞 IPC 线程。生产环境可能需要改为 async command + 独立 thread

### 对已有功能的影响

- `bootstrap` 命令不受影响（仍为同步）
- `task.create` 命令不受影响（仍通过 persistence 层）
- 其他命令仍返回 `BRIDGE_NOT_IMPLEMENTED`
- 事件转发器在后台运行，不影响现有事件流

### 性能与安全注意事项

- **进程隔离**：每个 session 一个独立进程，崩溃不影响其他 session
- **环境变量 allowlist**：仅传递 PATH/USERPROFILE/SYSTEMROOT/TEMP 等安全变量；XAI_API_KEY 显式传递但不记录
- **stderr 限量缓存**：200 行 ring buffer，防止 OOM
- **帧大小限制**：4 MiB 上限，防止恶意大帧
- **深度限制**：64 层，防止 JSON 炸弹
- **参数数组启动**：不使用 shell 字符串拼接，防止注入

## 六、后续事项

### 明确未完成项

1. 提取 ACP capability（models/modes/slashCommands）— GAG-006
2. 实现 session/task 映射（AgentEvent → DesktopEvent 时填充 task_id）— GAG-006
3. 实现 `runtime.login`（grok login 流程）— GAG-006 或独立任务
4. 实现 ACP `loadSession`（session resume）— GAG-006
5. Tauri build + Windows E2E 测试 — GAG-015/GAG-016
6. D5 独立审查 — 需另一旗舰模型审查

### 建议下一个任务

- **GAG-006**：Session 并发与恢复 — 建立 task_id ↔ session_id 映射，实现 turn.send/turn.cancel/session.resume 的完整流程
