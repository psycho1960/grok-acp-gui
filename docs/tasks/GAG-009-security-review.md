# GAG-009 安全实现记录

## 策略矩阵

| 操作 | 分类 | Plan 未批准 | 需要审批 | 持久 scope |
|---|---|---|---|---|
| 文件读取（路径完整且位于 Workspace） | read_only | 允许 | Agent 请求时按原 option | 仅精确 digest、限时 |
| `rg`/`fd`/`where` 参数数组 | read_only | 允许 | Agent 请求时按原 option | 仅精确 digest、限时 |
| Git `status/diff/log/show/rev-parse/ls-files` 与显式 list | read_only | 允许 | Agent 请求时按原 option | 仅精确 digest、限时 |
| 文件写入、Git add/commit/merge/switch | write | 拒绝 | 一次批准 | 禁止 |
| 文件删除、Git clean/reset、branch/worktree 删除 | destructive | 拒绝 | 一次批准 | 禁止 |
| 未知命令、shell 拼接、缺失字段、路径逃逸 | unknown | 拒绝 | 不可批准 | 禁止 |

## 威胁模型与封堵

- 伪造或串用审批：task、session、workspace、correlation、Plan version 和 operation digest 全绑定；任一不一致拒绝。
- 参数批准后变化：argv、cwd 和全部读写路径参与 SHA-256 digest，变化后没有匹配证据。
- 双击/竞态：后端 resolution mutex 串行 ACP 提交；SQLite 以 pending 状态条件更新，一次批准以条件 UPDATE 原子消费。
- 旧 Plan 批准复用：创建新版本的事务 supersede 旧 Plan，并 expire 旧 permission 记录。
- Renderer 伪造状态：UI 状态只用于展示；后端重新加载 SQLite 记录和当前 session binding 校验。
- 未知 ACP option：仅解析协议显式 kind/action；未知可展示但前后端都拒绝授权，绝不按 label 猜测。
- 敏感参数泄漏：session event、审计表与 Renderer 只接收脱敏参数/摘要；数据库保存 operation hash，不保存原始 argv。
- 进程退出、取消、超时：pending/一次批准失效；拒绝和超时不触发替代操作。
- 路径逃逸：Guard 拒绝 `..` 越界和 Workspace 外路径；实际 Adapter 仍须在 I/O 前 canonicalize 并检查 junction/reparse point。
- Adapter 绕过：当前 WorkspaceFilesystem 只有 canonicalized read；尚未存在 Git/文件写 Adapter。GAG-011～013 必须在实际 I/O 前调用同一 `ExecutionGuard`。

## 自动化证据

- Rust 单元测试：分类、未知默认拒绝、Plan 只读 gate、参数 digest、路径逃逸、一次消费。
- SQLite 集成测试：跨 session 拒绝、重复消费、过期、旧 Plan supersede 与批准失效。
- UI 测试：默认焦点拒绝、双击、多个 pending、超时、旧版本失效、键盘可操作按钮。
- 全仓扫描：现有文件 Adapter 无写接口；进程 Adapter 只负责受管 Grok 启动，不提供 Renderer 任意 Shell。

## 审查结论

实现自查未发现可直接绕过后端 gate 的现有写路径。剩余风险是 Windows junction/reparse point 必须由未来实际写 Adapter 在 I/O 瞬间再次验证，以及进程崩溃后需要按 session 恢复 expired 状态。按 Roadmap 的 D5 门禁，合并前仍须由不同旗舰模型独立复核；本记录不能替代该独立审查。
