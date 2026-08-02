# GAG-005：Grok Runtime 与 ACP 接入

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-005 |
| 类型 | 后端 / 外部进程 / ACP 协议 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | Grok 4.5 |
| 推荐 reasoning effort | High；涉及协议兼容或进程竞态时使用 XHigh |

可交给 GPT-5.6 Luna 或 DeepSeek V4 Flash 正式版的辅助工作：在协议契约稳定后补充 DTO 序列化测试、错误码快照和文档同步。两者均不得实现 ACP 解析、进程退出恢复或权限相关行为。

升级规则：Grok 4.5 遇到协议版本歧义、跨层状态不一致、进程竞态或安全边界问题时升级 GPT-5.6 Sol；同一验收项连续失败两次也必须升级。D5 合并前必须由 DeepSeek V4 Pro 或另一旗舰模型独立审查。

## 2. 背景与目标

应用必须通过 Grok Build 提供的 ACP/JSON-RPC 通道驱动 Agent，而不是解析 TUI 文本。本任务建立可启动、握手、收发消息、取消和关闭的 Grok Runtime，并把原始协议事件转换为稳定的内部事件。

目标是形成 `MOD-AGENT-RUNTIME` 的深接口：上层不感知子进程参数、JSON-RPC 帧、协议版本或 stderr 处理方式。

## 3. 需求映射

- PRD：FR-RUNTIME-001～005、FR-SESSION-001、FR-SESSION-004、NFR-SECURITY-001～003、NFR-RELIABILITY-001～002。
- UI：UI-ONBOARD-001、UI-CONV-001、UI-RECOVERY-001、UI-SETTINGS-001。
- 技术：MOD-AGENT-RUNTIME、ADP-GROK-ACP、DesktopBridge 事件 `agent:event`、`runtime:status_changed`。
- 路线图：阶段 2；前置 GAG-003、GAG-004。

## 4. 必读文档

1. 根 `AGENTS.md`：架构边界、ACP 与进程安全、日志规则。
2. `docs/01-PRD.md`：3.1、5.1、5.4、6。
3. `docs/02-UI-UX-DESIGN.md`：5.1、5.5、5.10、5.11。
4. `docs/03-TECHNICAL-DESIGN.md`：4～8、14。
5. `docs/04-AI-DEVELOPMENT-ROADMAP.md`：GAG-005 依赖与交付顺序。

## 5. 前置任务与开始条件

- GAG-003 已冻结 Bridge DTO、命令和事件版本。
- GAG-004 已提供运行时配置与会话记录的 Repository Interface。
- 已获得可执行的 Grok Build ACP 启动方式、版本探测方式和最小握手样例；若官方行为与技术方案冲突，停止并提交差异报告，不得猜测协议。
- Fake ACP agent 可作为测试适配器；没有生产适配器与测试适配器时不得新增无意义 seam。

## 6. 实现范围

- 定义 `AgentRuntime` Interface 和进程状态机。
- 实现 `grok_acp` Adapter：参数数组启动、stdin/stdout JSON-RPC、stderr 独立采集。
- 实现启动前探测、握手、能力协商、心跳/超时、取消和优雅退出。
- 将协议消息转换为内部 `AgentEvent`，保留关联 ID 与每会话单调序号。
- 对 JSON-RPC 帧设置大小、深度和字段校验；未知事件可审计但不得让进程崩溃。
- 输出结构化诊断信息，敏感字段脱敏。

## 7. 明确非范围

- 不实现任务并发调度、会话恢复策略或 UI 时间线。
- 不解析 ANSI/TUI 文本，不对自然语言日志做业务判断。
- 不实现权限批准或 Plan Mode 策略。
- 不下载、安装或自动升级 Grok Build。

## 8. 文件修改边界

允许：

- `src-tauri/src/modules/agent_runtime/**`
- `src-tauri/src/adapters/grok_acp/**`
- `src-tauri/src/bridge/**` 中 GAG-003 已定义接口的实现绑定
- `tests/fake-acp-agent/**`
- 与本任务直接对应的 Rust 测试和 fixtures

禁止：`src/features/**`、Git/Worktree Adapter、数据库 Migration、打包配置，以及未在任务说明书授权的重构。

## 9. Interface、DTO、事件与状态

`AgentRuntime` 至少提供：

- `probe(config) -> RuntimeProbeResult`
- `start(session_id, workspace, config) -> RuntimeHandle`
- `send(session_id, ClientRequest) -> RequestId`
- `cancel(session_id, request_id?)`
- `shutdown(session_id, reason)`

进程状态：`unavailable -> probing -> starting -> handshaking -> ready -> busy -> stopping -> stopped`；任意运行态可进入 `failed`。状态转换必须原子、可审计，非法转换返回领域错误。

内部事件至少包括：`session_ready`、`assistant_delta`、`assistant_completed`、`tool_started`、`tool_updated`、`tool_completed`、`plan_proposed`、`permission_requested`、`artifact_announced`、`request_failed`、`process_exited`。每个事件携带 `session_id`、`sequence`、`occurred_at` 和 `correlation_id`。

SQLite 影响：不得新增 Migration；仅通过 GAG-004 的 Repository Interface 读取运行时配置、写入必要的会话诊断摘要。

## 10. 实施顺序

1. 用 Fake ACP agent 固化正常握手、无效帧、超时、stderr 洪泛和异常退出 fixtures。
2. 实现纯协议编解码器与上限校验，再实现进程 Adapter。
3. 实现状态机及幂等 shutdown/cancel。
4. 接入 Bridge 事件发布，确保 Renderer 不接触原始 stdout。
5. 增加 Windows 路径、空格和 Unicode 工作目录测试。
6. 完成日志脱敏与退出诊断。

## 11. ACP、进程与安全不变量

- stdout 只能承载 ACP JSON-RPC；任何非协议内容作为协议错误，不得尝试 TUI 解析。
- 命令与参数必须以参数数组传递，禁止 shell 字符串拼接。
- stderr 不进入协议解码器；必须限量缓存，防止内存无界增长。
- 子进程继承环境变量必须使用 allowlist；密钥不得记录或传给 Renderer。
- 单一 session 只有一个受管进程句柄；重复停止和进程先行退出均应幂等。
- 超大帧、嵌套过深、非法 UTF-8、未知响应 ID 和重复结束事件不得破坏其他 session。

## 12. 异常与恢复

- 未安装、路径失效或版本不兼容：返回结构化探测结果和修复建议。
- 握手超时：终止受管进程，状态置 `failed`，保留限量诊断。
- 进程异常退出：发布 `process_exited`，不自动重放写操作。
- 半帧/坏帧：记录帧元信息并终止该 session；不得输出原始敏感正文。
- UI 订阅暂时断开：事件交由会话层持久化/重放，本模块不得无限缓冲。

## 13. 自动化测试

- 协议编解码 property/boundary tests。
- 状态机所有合法、非法和重复转换测试。
- Fake ACP 正常、超时、异常退出、错误 ID、乱序和 stderr 洪泛集成测试。
- Windows 带空格/中文路径进程启动测试。
- 日志脱敏测试，断言 token、Authorization、完整 prompt 不出现。
- 取消和 shutdown 的幂等、超时后强制回收测试。

## 14. 手工验收

1. 配置有效 Grok Build 可执行文件，探测显示版本与能力。
2. 启动会话并发送最小请求，UI 收到结构化增量和完成事件。
3. 强制结束进程，界面显示可恢复错误且应用不崩溃。
4. 配置无效路径，界面给出明确修复入口。
5. 检查日志，确认没有密钥、完整环境变量或原始 ACP 噪音。

## 15. Definition of Done

- 上述 Interface、状态、事件和安全不变量全部实现并有测试。
- 上层无 Grok CLI 参数、JSON-RPC 帧或进程句柄泄漏。
- 全部测试、Lint、类型检查和构建通过。
- D5 独立审查完成；交付报告包含模型、测试命令、结果、已知风险和协议依据。

## 16. 标准任务交付报告

报告必须列出：Task ID；实际模型与 reasoning；是否升级/降级及原因；修改文件；协议版本与能力；状态机变化；测试与手工证据；安全审查结论；剩余风险；未完成项。
