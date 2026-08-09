# GAG-005A 真实 Grok 验收

## 执行方式

真实测试必须显式开启，不在普通 CI 中隐式运行：

```powershell
$env:GROK_REAL_INTEGRATION = "1"
cargo test --manifest-path src-tauri/Cargo.toml --test gag_005_real_grok real_grok_ -- --test-threads=1 --nocapture
```

执行前使用官方流程 `grok login` 完成认证。应用和测试不读取、显示或保存 `auth.json`、Token 或 API Key。

## 验收项

1. `grok version` 探测到满足最低版本 `0.2.118` 的 CLI。
2. ACP 完成 `initialize`、`authenticate`、`session/new`。
3. 发送最小 Turn，收到非空 Assistant 回复；仅握手成功不等同于认证通过。
4. 长 Turn 能取消，保留已确认增量，且同一 session 可继续下一 Turn。
5. 关闭 Runtime/应用后所有受管登录和 ACP 子进程均退出。
6. 任一最小 Turn 返回 `GROK_AUTH_REQUIRED`/401 时，认证验收失败，禁止记录“真实集成完全通过”。

## 2026-08-09 本机结果

- Grok 路径：已探测（诊断不记录认证文件或秘密）。
- 版本：`1.0.0`，通过。
- ACP 启动/握手：通过。
- 进程关闭与幂等 shutdown：通过。
- 最小模型请求、只读工具请求、取消前的首个模型响应：返回 `GROK_AUTH_REQUIRED`，认证验收失败。
- 结论：真实集成未完全通过；需要重新执行官方 `grok login` 并在有效服务端认证下复测。

