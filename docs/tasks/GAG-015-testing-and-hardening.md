# GAG-015：系统测试、安全加固与发布门禁

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-015 |
| 类型 | 质量 / 故障注入 / 安全审计 |
| 难度 | D5 |
| 首选模型 | GPT-5.6 Sol |
| 备选模型 | DeepSeek V4 Pro |
| 推荐 reasoning effort | XHigh |

Luna 可在缺陷原因明确后补充回归用例、快照和测试报告；Flash 可完成机械性测试数据生成。不得让 Luna/Flash 独立判定安全边界、放宽断言或通过增加重试掩盖 flaky。

升级与审查：DeepSeek V4 Pro 发现跨层竞态、安全绕过或数据损坏风险时由 Sol 修复/裁决；本任务最终报告需要至少两个不同旗舰模型的审查记录。

## 2. 背景与目标

前序任务提供功能模块，本任务建立跨模块证据：正常流程、失败流程、崩溃恢复、路径安全、权限 fail-closed、Git 数据保全和性能均满足发布标准。发现缺陷时只做最小、可追踪修复，不顺手重构。

## 3. 需求映射

- PRD：全部 FR；重点 NFR-SECURITY-001～004、NFR-PERFORMANCE-001～002、NFR-RELIABILITY-001～002、NFR-ACCESSIBILITY-001～002、NFR-PRIVACY-001。
- UI：全部 UI screen，包括 UI-ERROR-001。
- 技术：全部 MOD/ADP、Bridge、Migration、Fake ACP、E2E、日志与打包前检查。
- 前置：GAG-001～014；GAG-016 以本任务门禁结果为输入。

## 4. 必读文档

- 根 `AGENTS.md` 全文。
- 四份主文档全文，尤其 PRD traceability、UI 状态、技术信任边界和路线图 Quality Gate。
- GAG-001～014 的 DoD 和交付报告。
- 实际依赖的官方 Tauri、Vue、SQLite、Git 与 ACP 版本文档；版本差异需记录。

## 5. 开始条件

- 所有前置任务已提交交付报告，未完成项已明确批准或阻止本任务。
- 测试能在隔离的临时目录/仓库运行，不接触真实用户项目。
- Fake ACP 可控制延迟、乱序、坏帧、权限、Artifact 和崩溃。
- 发布候选依赖已锁定；发现规格与实现冲突时停止并报告，不能自行选择。

## 6. 实现范围

- 建立需求 → UI → 模块 → 任务 → 测试的最终追踪矩阵。
- Rust 单元/集成、Vue 单元/组件、Bridge 契约、SQLite Migration、Fake ACP、Playwright E2E 测试完善。
- 故障注入：ACP/应用/Git/文件/数据库关键提交点。
- 安全审计：任意 Shell、路径逃逸、权限绕过、Plan 写门禁、XSS、秘密日志、恢复包。
- 并发、长会话、大任务列表、大 Artifact 的性能/资源测试。
- Windows 路径、高 DPI、键盘/屏幕阅读器与主题对比测试。
- flaky 隔离与根因修复；测试报告和发布门禁脚本。

## 7. 非范围与文件边界

非范围：新增产品功能、改变已批准 UX、升级无关依赖、性能重写。

优先允许：`tests/**`、各模块 `__tests__`/Rust tests、测试配置与 fixtures。若修复产品缺陷，可修改对应任务原授权目录，但交付报告必须逐项映射缺陷和原因。

禁止：删除失败测试、降低安全断言、无依据扩大超时/重试、修改已合并 Migration、在真实项目执行破坏性 E2E。

## 8. 测试矩阵

测试 Harness Interface 包含 `FakeDesktopBridgeScenario`、`FakeAcpScript`、`TempRepositoryFixture`、`TempSqliteFixture`、`FaultInjector` 和 `EvidenceRecorder`；这些测试 seam 必须确有生产 Adapter 对应，且只能指向临时隔离资源。

最低层级：

- Contract：Bridge command/event schema、序列化与版本兼容。
- Domain：状态机、权限、路径证明、Git 算法、事务。
- Adapter integration：Fake ACP、真实 Git 临时仓库、SQLite 临时库、文件系统 fixtures。
- Component：所有 UI 状态、键盘、焦点、错误边界。
- E2E：onboarding → 项目 → 任务 → 会话/工具 → 权限/Plan → Artifact → Review/Checkpoint → Squash → Cleanup/Recovery。
- Resilience：断电等价 kill、进程退出、磁盘满/只读、数据库 busy/corrupt 副本、Git 冲突。

每个 PRD requirement 至少有一个自动化测试或说明为何只能手工验证，并指定证据负责人。

## 9. 固定安全检查

- 静态搜索 Renderer 中的 Tauri invoke 绕过、shell 字符串和秘密标记。
- 动态证明所有写 I/O 通过 ExecutionGuard。
- 路径 fuzz：`..`、大小写、Unicode、junction/symlink、同前缀目录、长路径。
- Git 不变量：主工作树不变、未选文件不提交、目标 ref CAS、删除前恢复包。
- ACP：stdout 仅 JSON-RPC、帧上限、未知事件、stderr 上限、环境 allowlist。
- WebView：Markdown/链接/Artifact 主动内容隔离和 CSP。
- 日志/数据库/崩溃报告秘密扫描。

## 10. 性能与可靠性门槛

若主文档没有更严格值，候选门槛为：500 任务列表保持可交互；10,000 事件时间线不一次性挂载全部 DOM；100 个高频 delta burst 不乱序；应用冷启动和常用切换记录 P50/P95；多 session 压力不产生跨会话事件。

性能数据必须注明硬件、构建模式、样本量和阈值。阈值未达成不得仅以“主观流畅”通过。

## 11. 推荐实施顺序

1. 汇总追踪矩阵和测试缺口，先覆盖 D5 安全路径。
2. 建隔离 harness、Fake ACP 场景和临时 Git/SQLite fixtures。
3. 完成正常 E2E，再逐个关键点故障注入。
4. 做静态/动态安全审计和秘密扫描。
5. 执行性能、长时和无障碍测试。
6. 最小修复缺陷并增加回归测试。
7. 生成可复现的门禁命令和最终报告。

## 12. 自动化测试与 CI 门禁

- Rust：format、lint、unit、integration、Migration、并发/故障测试。
- Renderer：format、lint、typecheck、unit、component、accessibility。
- E2E：Windows 关键路径、失败路径、恢复路径。
- 构建：debug 和 release candidate。
- 禁止只因 flaky 重跑通过；同用例非确定失败必须有 issue、owner 和发布裁决。
- CI 日志需保留命令、退出码、版本和 Artifact，且经过脱敏。

## 13. 手工验收

- 按 PRD 十条核心用户旅程逐条执行并留证。
- 键盘-only、高 DPI、窗口最小尺寸、离线/断线和恢复。
- 使用副本仓库验证冲突、目标分支前进、dirty Worktree 和恢复包。
- 检查 Windows 安装前的权限提示、日志目录和数据目录行为。

## 14. Definition of Done

- 所有 requirement 可追踪，P0/P1 缺陷为零；其余缺陷有明确风险接受。
- 安全不变量、故障注入、Migration、性能、无障碍和 E2E 门禁通过。
- 没有通过禁用测试、放宽断言或增加盲目重试掩盖问题。
- 双旗舰独立审查完成并记录分歧与结论。
- 形成 GAG-016 可直接使用的发布候选报告。

## 15. 标准任务交付报告

包含 Task ID、实现/审查模型与 reasoning、需求覆盖率、测试环境、全部命令/退出码、故障注入矩阵、安全发现与修复、性能数据、无障碍结果、缺陷清单、发布建议与风险接受人。
