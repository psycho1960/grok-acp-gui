# GAG-003：DesktopBridge 契约、DTO、事件与错误模型

## 1. 任务元数据

- Task ID：GAG-003
- 类型：feat/architecture
- 难度：D4
- 首选模型：GPT-5.6 Sol，reasoning `max`
- 备选模型：DeepSeek V4 Pro，Thinking mode 开启
- 推荐 reasoning effort：Max；使用 DeepSeek V4 Pro 时开启 Thinking mode
- 前置任务：GAG-001
- 可并行：可与 GAG-002 并行；独占 Bridge 公共类型

Luna 或 Flash 只可同步 Rust/TS enum 名称、生成 fixture 和测试表。低成本模型同一验收项连续失败两次升级 Terra；若不能保持 Rust/TS 一致、命令面暴露任意路径/命令、出现跨层状态不一致或需要改变深模块数量，必须由 Sol 继续并记录 ADR。

## 2. 目标

定义 Renderer 唯一后端 seam：一个 `bootstrap`、一个命令联合入口和一个事件订阅入口。锁定 DTO、状态名、错误模型、序列化规则和 capability，避免后续 Feature 直接依赖 Tauri/ACP/Git/SQLite。

## 3. 规范映射

- PRD：所有 FR 的 Renderer 可见结果；NFR-SECURITY-001/003
- UI：所有 Screen 的数据/动作需求
- 技术：第 4–6、13 节，SEAM-DESKTOP-BRIDGE

## 4. 必读材料

- 三份主规格全文，尤其技术第 5、6、13 节
- ACP SDK schema 的 session、permission、content、config types
- Tauri 2 command/event serialization 规则

开始条件：GAG-001 基线可构建，PRD requirement 与 UI screen 清单已冻结，Rust/TypeScript 生成或 fixture 校验工具可用。若 ACP SDK、Tauri 版本或主文档之间存在字段冲突，停止并提交 Interface 差异报告，不通过增加 `any` 或原始 JSON 逃避决策。

## 5. 实现范围

- TypeScript `DesktopBridge`、`DesktopCommand`、`DesktopResult`、`DesktopEvent`。
- Rust 对应 command/result/event DTO 与 serde tagging。
- ID newtype/string 规则、时间戳、路径展示与内部路径分离。
- `AppError` code/message/action/retryable/detailsRedacted/correlationId。
- Tauri command 只暴露 `bootstrap`、`execute`；事件使用单一受控 channel/topic。
- Renderer bridge client；测试 FakeDesktopBridge。
- 契约 round-trip/fixture 测试，防止 Rust/TS 漂移。

## 6. 非范围

- 不实现 Grok、Git、SQLite 或业务状态机。
- 不为每个动作定义独立 Tauri command。
- 不暴露 child process、SQL、shell、absolute cache path 或认证信息。

## 7. 允许修改

- `src/bridge/**`
- `src-tauri/src/bridge/**`
- `src-tauri/src/domain/error*` 与必要公共 ID/DTO
- composition root 中注册命令/事件的最小修改
- Bridge 契约测试和 fixtures

禁止修改 Feature UI、Workspace/Grok 实现和 Migration。

## 8. Interface 决策

```ts
interface DesktopBridge {
  bootstrap(): Promise<BootstrapSnapshot>;
  execute(command: DesktopCommand): Promise<DesktopResult>;
  subscribe(listener: (event: DesktopEvent) => void): Promise<Unsubscribe>;
}
```

`DesktopCommand` 使用 `{ type, payload }` 判别联合；`DesktopEvent` 使用 `{ type, taskId?, sessionId?, seq?, timestamp, payload }`。会话事件必须包含 `taskId/sessionId/seq`，非会话 Runtime 事件不伪造 seq。

命令类别按技术方案第 5 节固定。权限/Plan resolve 携带后端生成 request ID 与 ACP option ID；不得传原始 JSON-RPC。

## 9. 验证与错误规则

- Bridge 验证空 ID、超长文本、非法 enum、非有限数值、路径编码和 payload 大小。
- 文件/图片本体不通过 JSON payload；使用 Artifact ID。
- 未识别 command 返回 `BRIDGE_UNSUPPORTED_COMMAND`，不 panic。
- 错误 details 默认脱敏；UI 可复制 correlation ID。
- Event listener 取消订阅必须幂等。

## 10. 推荐实施顺序

1. 从 PRD/UI 提取 Renderer 必需动作和数据。
2. 定义语言无关的契约文档/fixture。
3. 实现 Rust DTO/serde 和 TypeScript 类型。
4. 实现单 command dispatcher 与事件 channel。
5. 实现前端 client 和 FakeDesktopBridge。
6. 加入 round-trip、未知类型、错误与取消订阅测试。

## 11. 自动化测试

- 每个 command/result/event Rust serialize → fixture → TS parse。
- 非法/缺字段 payload 返回稳定错误。
- 大文本/图片 Base64 被拒绝。
- listener 多订阅、取消、重复取消和应用关闭。
- 静态依赖检查：Feature 不得导入 Tauri API；Bridge 不得导出 shell/fs/sql。

## 12. 手工验收

- DevTools 可通过 FakeDesktopBridge 展示 bootstrap 和事件。
- 未运行后端时错误可读且包含恢复动作。
- 查看 Renderer 全局对象不存在任意 execute shell 能力。

## 13. Definition of Done

- Rust/TS 契约测试全绿且命名一致。
- DesktopBridge 是 Renderer 唯一后端 seam。
- 错误和事件顺序不变量有文档与测试。
- 后续任务无需新增 Tauri command 即能扩展联合类型。
- 安全审查确认无任意 Shell/路径/SQL 暴露。

## 14. UI、SQLite 与外部交互影响

- UI 只使用 FakeDesktopBridge 做契约演示；本任务不定义最终页面布局，也不复制领域状态推导。
- SQLite Migration 影响为无；Bridge 只能定义持久化模块需要实现的 DTO，不创建表或执行 SQL。
- ACP、Git、文件和进程均只以类型化意图/结果出现，不调用真实 Adapter；Artifact 二进制使用 ID/受控流式资源协议，不进入 JSON 事件。
- 事件订阅必须支持 snapshot/cursor 的后续扩展，且 Renderer 断开不能反向阻塞后端进程读取。

## 15. 标准任务交付报告

报告必须包含：Task ID；实际模型、reasoning 与升级记录；修改文件；command/result/event/error 清单；Rust/TS fixture 版本；安全面审查；契约、类型检查、Lint 与构建退出码；破坏性变更；后续任务需遵守的 Interface 冻结点。
