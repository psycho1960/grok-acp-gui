# GAG-015 测试与加固交付报告

## 一、任务完成情况

- Task ID：GAG-015。
- 已完成：独立任务分支；最终追踪矩阵；确定性 release gate；Renderer/进程/CSP/秘密固定静态检查；500 tasks、10k timeline、100 delta burst 性能门槛；现有 Rust/Bridge/Migration/Fake ACP/Git/Recovery 测试汇总。
- 未完成：GAG-001～014 的独立历史交付报告在仓库中不存在，GAG-015 开始条件的该项未满足；十条 Windows 手工用户旅程、原生强制关闭/高 DPI/屏幕阅读器证据待执行；不同旗舰模型独立审查未执行。因此本报告不能给出最终发布放行结论。
- 与计划差异：不补写历史报告、不虚构模型审查；不做用户要求之外的独立安全审查。固定安全检查仅按 GAG-015 第 9 节实现为可复现门禁。
- 实际使用模型及升级/降级：Codex GPT-5 系列；当前运行环境未暴露可核验的精确 served model ID，不能声明为任务首选的 GPT-5.6 Sol。未调用辅助/审查模型。

## 二、测试环境与版本依据

- 操作系统：Windows 11 专业版 `10.0.22631` x64；Intel Core i9-14900HX（24 core / 32 logical processor）；物理内存 68,402,393,088 bytes。构建模式为 Vite production、Rust dev tests 与 Tauri release candidate。
- 本地基线：Node `v24.15.0`、npm `11.12.1`、Rust/Cargo `1.97.1`、Git `2.51.0.windows.1`。
- Tauri：实际 Rust `2.11.5` / JS API `2.11.1`。[官方 capability 文档](https://v2.tauri.app/security/capabilities/)确认 capability 只缩小 WebView 暴露面，不能替代 Rust 业务授权；[官方 CSP 文档](https://v2.tauri.app/security/csp/)要求只允许可信来源。本任务因此同时验证 capability、CSP 与后端 fail-closed 测试。
- Vue：实际 `3.5.27`。[官方安全文档](https://vuejs.org/guide/best-practices/security)要求不把不受信任内容当模板/未清洗 HTML；项目仅允许 `SafeMarkdown.vue` 使用经过 `renderSafeMarkdown` 清洗的 `v-html`，并由主动内容测试覆盖。
- SQLite：实际 `rusqlite 0.31.0` bundled SQLite。[官方事务文档](https://sqlite.org/lang_transaction.html)说明 `BEGIN IMMEDIATE` 会立即取得写事务并可能返回 `SQLITE_BUSY`；现有事务、busy、Migration 与恢复测试作为证据。
- Git：本机 `2.51.0.windows.1`；[官方 `git-worktree` 文档](https://git-scm.com/docs/git-worktree)标明 2.48.1～2.51.2 在该手册无差异，[`git-update-ref` 文档](https://git-scm.com/docs/git-update-ref)规定三参数形式先匹配 old object 再更新。
- ACP：项目锁定 `@agentclientprotocol/sdk 0.13.1`，[官方 npm 包](https://www.npmjs.com/package/@agentclientprotocol/sdk)当前版本已高于该版本；[官方协议仓库](https://github.com/agentclientprotocol/agent-client-protocol)要求以协商的 protocolVersion 和 capability 判定兼容性。根据任务“禁止升级无关依赖”，本任务不升级 SDK；版本差异保留为 GAG-016 前的兼容性风险。

## 三、修改文件与 Interface / 数据变化

- 新增：GAG-015 harness、性能/安全测试、静态门禁、release gate、追踪矩阵和本报告。
- 修改：`package.json` 测试命令、`playwright.config.ts` worker 上限。
- 删除：无。
- DesktopBridge/事件/DTO：无变化。
- SQLite Migration：无变化，既有 Migration 未改写。
- 状态机：无变化。
- 生产依赖：无变化。
- 回滚方式：移除新增测试/脚本/文档，并恢复两个测试配置文件；无数据回滚。

## 四、测试结果

- 基线 TypeScript 类型检查、Lint、Rust fmt：通过。
- 基线 Node：29/29；Vitest：208/208。
- 基线 Playwright：33/34；唯一失败为 9 worker 同时启动 Chromium 时 `browserType.launch` 超时。4 worker 单独运行曾 34/34，但在完整门禁负载下再次出现同类启动超时，证明并发启动仍不稳定；根因修复为固定 1 worker、`retries: 0`，未扩大超时。
- 基线 Rust clippy：通过；Rust unit/integration：全部通过（库测试 226 个，GAG-005～014 集成族通过）。
- GAG-015 完整门禁：`npm run gate:gag015` 退出码 0，总耗时 155.00 秒；12 个步骤全部通过。机器可读证据写入 `.gag-015-evidence/*.json`，该目录不提交。
- ACP 契约测试：由 Fake ACP codec/runtime/production bridge 测试覆盖；真实 Grok 仍为 guarded live，不作为普通 CI 的真实性门禁。
- Git 集成测试：只使用临时仓库，覆盖路径、精确暂存、CAS、冲突、恢复包和清理。
- Windows 浏览器 E2E：Playwright 34/34，通过 1024/1200/1440、200% 页面缩放、axe、键盘和核心 fixture 流；原生 Tauri/安装包手工旅程尚未执行。
- Tauri Build：通过；生成 unsigned NSIS 与 MSI 候选包。构建产物位于被忽略的 `src-tauri/target/release/bundle`，未提交。

## 五、故障注入、安全、性能与无障碍

- 故障注入：Fake ACP bad frame/crash/timeout/stderr flood；文件复制中断/磁盘满；SQLite busy/corrupt/migration；Git ref 前进/冲突/commit failure/清理中断；恢复包损坏。
- 固定安全检查：Renderer 直接 Tauri/shell、Rust shell 字符串、ACP stdout 日志、Tauri capability、CSP、Markdown 主动内容与 credential literal 扫描。
- 性能阈值与单次样本：500 tasks 分组 `2.73 ms < 500 ms`，504 个逻辑行只挂载 `18 < 50` DOM 行；10k events `928.69 ms < 5,000 ms`，既有组件测试同时证明 timeline DOM 少于 100；reversed 100 delta burst 最终连续到 seq 100、无残留 pending event。
- 性能样本：每项单次确定性 CI 门禁样本；不是容量规划 benchmark。P50/P95 冷启动、常用切换和多 session 长时压力仍需 Windows 候选构建采样。
- 无障碍：现有键盘、焦点、accessible name、axe、200% 页面缩放与 reduced-motion 测试继续作为自动化门禁；真实屏幕阅读器和 Windows 高 DPI 为手工项。

## 六、风险与发布建议

- P0/P1 自动化回归：完整自动化门禁未发现失败；手工与独立审查门禁仍未满足。
- 对已有功能的影响：仅测试执行并发从自动 9 worker 限制为 1；不改变产品运行时。
- 安全注意事项：本任务没有扩大安全策略或修改生产授权路径，只把已批准不变量固化成静态/动态证据。
- 明确未完成项：历史交付报告、双旗舰审查、Windows 原生十条旅程、安装包权限/目录行为、P50/P95 与长时压力证据；CI Artifact 外传未获授权，JSON 证据仅保留本地。
- 发布建议：当前为“门禁证据建设中 / 不放行”；只有完整自动化、手工证据和任务要求的外部审查均完成后，GAG-016 才可把它作为发布候选输入。
