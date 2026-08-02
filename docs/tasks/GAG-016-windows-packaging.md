# GAG-016：Windows 打包、安装与升级

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-016 |
| 类型 | Windows / Tauri / 发布工程 |
| 难度 | D3 |
| 首选模型 | GPT-5.6 Terra |
| 备选模型 | DeepSeek V4 Flash 正式版 |
| 推荐 reasoning effort | Medium；签名、升级或数据迁移异常时 High 并升级 Sol |

Luna 可更新版本号、发布说明、校验和文档；Flash 可处理确定性打包配置和 CI 矩阵。遇到签名信任、自动更新供应链、安装/卸载数据丢失或 Migration 失败时升级 GPT-5.6 Sol。

## 2. 背景与目标

应用最终面向 Windows 用户，需要可安装、可启动、可升级且不会误删项目/恢复数据的包。本任务完成 Tauri release 构建、NSIS/MSI 策略、签名接口、安装/升级/卸载验证和发布清单。

## 3. 需求映射

- PRD：FR-RUNTIME-001～002、FR-PROJECT-001、FR-RECOVERY-001，NFR-SECURITY-001～004、NFR-RELIABILITY-001～002、NFR-PRIVACY-001。
- UI：UI-ONBOARD-001、UI-SETTINGS-001、UI-ERROR-001。
- 技术：Tauri 配置、CSP、数据目录、日志、Migration、Windows 打包与签名。
- 前置：GAG-001～015，尤其 GAG-015 发布候选门禁通过。

## 4. 必读文档

- `AGENTS.md`：密钥、构建、证据和任务范围。
- `01-PRD.md` 5.1、5.11、6、8。
- `02-UI-UX-DESIGN.md` 5.1、5.11 和错误/恢复状态。
- `03-TECHNICAL-DESIGN.md` 2～4、11、14～16。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 5、Quality Gate。
- GAG-015 最终报告和风险接受项。

## 5. 开始条件

- release candidate commit 固定，工作区干净，依赖锁文件已审查。
- 应用 ID、显示名、版本、publisher、图标和最低 Windows 版本已确定。
- 是否提供 NSIS、MSI 或两者按技术文档执行；若签名证书/发布渠道缺失，可完成无签名内部候选，但不得声称生产就绪。
- 签名秘密只通过 CI/安全环境注入，不写入仓库或日志。

## 6. 实现范围

- Tauri release 配置、bundle metadata、图标、CSP 和 allowlist 最终核对。
- NSIS/MSI 构建产物及架构矩阵（技术方案规定的 x64；其他架构需单独批准）。
- 安装目录、应用数据、日志、Artifact、Worktree 与 recovery root 的明确归属。
- 首装、覆盖升级、跨 schema 升级、降级拒绝、卸载和重装测试。
- Grok Build 缺失/路径变更的 onboarding 体验。
- 代码签名/时间戳接口、校验和与 SBOM/依赖清单（发布渠道支持时）。
- 发布说明、已知限制和回滚手册。

## 7. 非范围与文件边界

非范围：上架 Microsoft Store、自动下载 Grok Build、远程自动更新服务，除非另有批准任务。

允许：Tauri/bundle/CI/installer 配置、图标与发布文档、打包测试。产品代码仅可修改由安装/升级验证发现且属于本任务的 Windows 启动或路径适配问题。

禁止：提交证书、密码、token；卸载时删除用户项目、外部 Git repo、受管 Worktree 或 recovery bundle；静默忽略 Migration 失败。

## 8. 安装与数据规则

- 安装层不新增 Renderer Interface；仅通过既有 DesktopBridge `bootstrap/get_diagnostics/update_settings` 暴露版本与诊断 DTO，禁止安装器向 Renderer 注入命令能力。
- 程序二进制安装在系统认可位置；可变数据位于版本无关的应用数据目录。
- 用户项目永不复制或移入安装目录。
- 升级前数据库执行既有 Migration；失败时保留原数据和可诊断备份，不启动半迁移版本。
- 卸载默认移除程序文件，保留数据库、设置、受管 Artifact、Worktree 与恢复包；如提供删除数据选项，必须单独明确确认并列出路径。
- 降级检测到更高 schema version 时拒绝写入并提供恢复说明。
- SQLite Migration 只能执行 GAG-004 及后续任务已合并的不可变脚本；本任务不新设计业务 Schema。升级测试必须同时保留原数据库副本与 Migration 证据。

## 9. UI 与错误流

首次启动 `UI-ONBOARD-001` 探测 Grok Build、Git 与可写数据目录；缺失项提供手动路径和验证，不自动执行未知下载。

`UI-SETTINGS-001` 显示应用版本、数据/日志/恢复目录和运行时版本；复制诊断信息必须脱敏。

`UI-ERROR-001` 覆盖 Migration 失败、数据目录不可写、Runtime 不兼容和 WebView/runtime 缺失，提供安全退出和打开诊断目录。

## 10. 推荐实施顺序

1. 冻结版本与 bundle metadata，核对 CSP/权限。
2. 在干净 Windows 环境构建并安装候选包。
3. 执行首次安装、卸载保留数据、重装。
4. 从至少一个前一 schema fixture 执行覆盖升级和回滚演练。
5. 接入签名/时间戳，验证签名和校验和。
6. 完成干净 VM 与标准用户权限验收。
7. 输出产物 manifest、发布说明与回滚手册。

## 11. 安全与可靠性不变量

- 构建日志和产物不含签名秘密、API token 或开发机绝对路径。
- 安装/卸载不执行任意用户提供命令。
- 卸载默认不删除用户数据和工作成果。
- 应用权限维持最小化，Renderer 不新增 shell/文件系统直通能力。
- 每个产物有 SHA-256、版本、架构、commit SHA 和签名状态。
- Migration 失败或数据目录不可写时 fail closed，不创建新的空库掩盖原数据。

## 12. 自动化测试

- release build、bundle 结构、CSP/allowlist 静态检查。
- 干净安装、静默参数（若支持）、启动、卸载退出码测试。
- 数据目录含空格/Unicode、标准用户权限和长路径测试。
- 前一 schema 升级、失败注入、降级拒绝测试。
- 卸载后用户 repo/Worktree/Artifact/recovery 数据保留断言。
- 签名验证、SHA-256 manifest 和依赖清单测试。

## 13. 手工验收

1. 干净 Windows VM 以标准用户安装并启动。
2. Grok Build 缺失时 onboarding 可完成诊断，不出现白屏。
3. 用已有数据升级后，项目、任务、会话和 Artifact 可访问。
4. 卸载后验证所有用户数据与 Worktree 未被删除。
5. 重装后重新识别已有数据；签名与版本信息正确。

## 14. Definition of Done

- 约定的 Windows 安装包构建成功并通过干净环境验证。
- 安装、升级、Migration 失败、卸载保留和重装流程有证据。
- 签名状态、校验和、产物 manifest、发布说明和回滚手册齐全。
- GAG-015 门禁仍通过；未因打包放宽安全边界。
- 交付报告明确区分“内部无签名候选”和“已签名生产候选”。

## 15. 标准任务交付报告

包含 Task ID、模型/reasoning、修改文件、构建环境、产物名称/大小/SHA-256/签名、安装升级矩阵、Migration 结果、卸载数据验证、测试命令/退出码、已知限制、发布与回滚建议。
