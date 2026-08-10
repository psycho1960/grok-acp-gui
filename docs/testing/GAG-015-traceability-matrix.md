# GAG-015 最终追踪矩阵

本矩阵只汇总既有产品需求与自动化证据，不新增功能、UX 或安全规则。自动化证据均使用进程内 Fake、仓库内 Fake ACP 或操作系统临时目录；不得指向真实用户项目。

| Requirement | UI | Module / Adapter | 主责 Task | 自动化测试或手工证据 | 证据负责人 |
|---|---|---|---|---|---|
| FR-RUNTIME-001、002、004、005 | UI-ONBOARD-001、UI-ERROR-001 | MOD-AGENT-RUNTIME、ADP-GROK-ACP | GAG-003～005 | `gag_005_p0_probe.rs`、`gag_005_runtime_integration.rs`；安装/登录窗口提示保留给 Windows 手工验收 | GAG-015 自动化；GAG-016 手工 |
| FR-RUNTIME-003 | UI-ONBOARD-001、UI-CONV-001 | MOD-AGENT-RUNTIME、ADP-GROK-ACP | GAG-005 | `gag_005_runtime_integration.rs`、`gag_005_real_grok.rs`、codec 单元测试 | GAG-015 自动化 |
| FR-PROJECT-001～004 | UI-PROJECT-001、UI-TASK-001、UI-WORKTREE-001 | MOD-TASK-RUNTIME、MOD-WORKSPACE、MOD-PERSISTENCE | GAG-004、007、011 | `gag-007-closed-loop.spec.ts`、`gag_011_worktree_lifecycle.rs`、SQLite repository 单元测试 | GAG-015 自动化 |
| FR-TASK-001～003 | UI-TASK-001、UI-TASK-002 | MOD-TASK-RUNTIME、MOD-PERSISTENCE | GAG-004、006、007 | `gag-007-store.spec.ts`、`gag_006_concurrency_and_recovery.rs`、领域状态穷举测试 | GAG-015 自动化 |
| FR-TASK-004 | UI-TASK-001、UI-CONV-001 | MOD-TASK-RUNTIME | GAG-006、007 | `gag_006_semaphore_leak_repro.rs`、runtime concurrency limit 单元测试 | GAG-015 自动化 |
| FR-SESSION-001～005 | UI-CONV-001、UI-RECOVERY-001 | MOD-AGENT-RUNTIME、MOD-TASK-RUNTIME | GAG-005、006、008 | `gag_005_runtime_integration.rs`、`gag_006_concurrency_and_recovery.rs`、`gag_008_production_bridge.rs`、`gag-008-reducer.spec.ts` | GAG-015 自动化 |
| FR-PERMISSION-001～003 | UI-PERM-001、UI-CONV-001 | MOD-TASK-RUNTIME、MOD-AGENT-RUNTIME | GAG-009 | `gag_009_permissions_plan.rs`、`gag_009_release_gate_integration.rs`、`gag-009-permissions-plan.spec.ts`、`gag-009.spec.ts` | GAG-015 自动化 |
| FR-PLAN-001～002 | UI-PLAN-001、UI-CONV-001 | MOD-TASK-RUNTIME、MOD-AGENT-RUNTIME | GAG-009 | `gag_009_plan_permission_e2e.rs`、`gag_009_release_gate_integration.rs` | GAG-015 自动化 |
| FR-IMAGE-001～003、005 | UI-ARTIFACT-001、UI-CONV-001 | MOD-ARTIFACTS、MOD-PERSISTENCE、ADP-FILESYSTEM | GAG-010 | Artifact 模块单元测试、`gag_010_luna_handoff.rs`、`gag_010c_artifact_save.rs` | GAG-015 自动化 |
| FR-IMAGE-004 | UI-ARTIFACT-001 | MOD-ARTIFACTS、ADP-FILESYSTEM | GAG-010 | `gag-010c-artifact-save.spec.ts`、`gag_010c_artifact_save.rs`；资源管理器窗口定位保留给 Windows 手工验收 | GAG-015 自动化；GAG-016 手工 |
| FR-WORKTREE-001～005 | UI-WORKTREE-001、UI-RECOVERY-001 | MOD-WORKSPACE、ADP-GIT-CLI、MOD-PERSISTENCE | GAG-011、014 | `gag_011_worktree_lifecycle.rs`、`gag-011-worktree-lifecycle.spec.ts`、`gag_014_recovery_center.rs` | GAG-015 自动化 |
| FR-REVIEW-001～003 | UI-REVIEW-001 | MOD-WORKSPACE、ADP-GIT-CLI | GAG-012 | `gag_012_diff_checkpoints.rs`、`gag-012-review.spec.ts` | GAG-015 自动化 |
| FR-REVIEW-004～005 | UI-WORKTREE-001、UI-RECOVERY-001 | MOD-WORKSPACE、ADP-GIT-CLI、MOD-PERSISTENCE | GAG-013、014 | `gag_013_squash_integration.rs`、`gag-013-integration.spec.ts` | GAG-015 自动化 |
| FR-RECOVERY-001～003 | UI-RECOVERY-001 | MOD-WORKSPACE、MOD-ARTIFACTS、MOD-PERSISTENCE | GAG-006、011、014 | `gag_011_worktree_lifecycle.rs`、`gag_013_squash_integration.rs`、`gag_014_recovery_center.rs`、`gag-014-recovery-center.spec.ts` | GAG-015 自动化 |
| NFR-SECURITY-001 | 全部 Renderer | DesktopBridge、Tauri capability | GAG-003、009、015 | `gag-015-static-gate.mjs`、`bridge-contracts.test.ts` | GAG-015 自动化 |
| NFR-SECURITY-002 | Worktree、Artifact、Recovery | MOD-WORKSPACE、MOD-ARTIFACTS、ADP-FILESYSTEM | GAG-010～015 | filesystem path/junction 单元测试、`gag_011_worktree_lifecycle.rs`、`gag_013_squash_integration.rs`、`gag_014_recovery_center.rs` | GAG-015 自动化 |
| NFR-SECURITY-003 | UI-ERROR-001、诊断 | MOD-AGENT-RUNTIME、日志 | GAG-005、009、015 | diagnostics/mailbox redaction 单元测试、`gag-009-console-redact.spec.ts`、`gag-015-static-gate.mjs` | GAG-015 自动化 |
| NFR-SECURITY-004 | UI-WORKTREE-001、UI-RECOVERY-001 | MOD-WORKSPACE | GAG-011、013、014 | destructive prepare/execute 负面测试与 Recovery Center 组件测试 | GAG-015 自动化 |
| NFR-PERFORMANCE-001 | UI-TASK-001、UI-CONV-001、UI-ARTIFACT-001、UI-REVIEW-001 | Renderer Features | GAG-007、008、010、015 | `gag-015-performance-security.spec.ts`（500 tasks）、`gag-008-components.spec.ts`（10k timeline DOM） | GAG-015 自动化 |
| NFR-PERFORMANCE-002 | UI-CONV-001 | MOD-TASK-RUNTIME、Renderer reducer | GAG-006、008、015 | `gag-015-performance-security.spec.ts`（reversed 100 delta burst）、`gag-008-reducer.spec.ts`（10k events） | GAG-015 自动化 |
| NFR-RELIABILITY-001 | UI-ERROR-001、UI-RECOVERY-001 | MOD-AGENT-RUNTIME | GAG-005、006、014 | `gag_005_runtime_integration.rs` shutdown/crash/stderr tests；真实应用强制关闭保留给 Windows 手工验收 | GAG-015 自动化；GAG-016 手工 |
| NFR-RELIABILITY-002 | UI-ERROR-001 | MOD-PERSISTENCE、ADP-SQLITE | GAG-004、006、014 | migration checksum/fresh/upgrade/corrupt/busy 与事务单元/集成测试 | GAG-015 自动化 |
| NFR-ACCESSIBILITY-001 | 全部 UI | Renderer App Shell / Features | GAG-002、007～010、015 | Playwright axe、组件键盘/焦点测试、`gag-015-performance-security.spec.ts` list semantics；screen reader 流程为手工证据 | GAG-015 自动化；GAG-016 手工 |
| NFR-ACCESSIBILITY-002 | 全部 UI | Design Token / Renderer | GAG-002、015 | `check-theme-tokens.mjs`、Playwright axe；Windows 高 DPI 与系统对比度为手工证据 | GAG-015 自动化；GAG-016 手工 |
| NFR-PRIVACY-001 | UI-SETTINGS-001、UI-ERROR-001 | 全部 Adapter 与构建配置 | GAG-001、005、010、015 | `gag-015-static-gate.mjs`、依赖/源码遥测静态测试、日志/数据库敏感数据单元测试 | GAG-015 自动化 |

## Harness Interface 对应关系

| Harness Interface | 实现/使用位置 | 对应生产 seam | 隔离约束 |
|---|---|---|---|
| `FakeDesktopBridgeScenario` | `tests/harness/gag-015-harness.ts` | `DesktopBridge` | 仅进程内 listener/command handler |
| `FakeAcpScript` | TypeScript 描述 + Rust `FakeAcpTransport/FakeScenario` | `AcpTransport` | 仅仓库内 `tests/fake-acp-agent/agent.mjs` |
| `TempRepositoryFixture` | GAG-011～014 Rust integration fixtures | `GitCli` / `WorkspaceModule` | `std::env::temp_dir()` 下随机目录 |
| `TempSqliteFixture` | GAG-004～015 Rust tests | `SqliteRepository` / Repository Interface | 内存库或系统临时文件 |
| `FaultInjector` | filesystem、ACP、integration、recovery 故障测试 | 现有 Adapter seam | 只在临时副本注入 |
| `EvidenceRecorder` | `tests/harness/gag-015-harness.ts` 与 release gate JSON | 测试报告层，无生产权限 | 只写被 `.gitignore` 排除的 `.gag-015-evidence` |
