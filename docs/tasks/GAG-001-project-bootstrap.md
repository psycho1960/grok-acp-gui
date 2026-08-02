# GAG-001：工程基线、Fork 与产品清理

## 1. 任务元数据

- Task ID：GAG-001
- 类型：chore/build
- 难度：D3
- 首选模型：DeepSeek V4 Flash 正式版，Thinking mode 开启
- 备选模型：GPT-5.6 Terra，reasoning `medium`
- 推荐 reasoning effort：DeepSeek Thinking mode；使用 Terra 时为 Medium
- 前置任务：无
- 后续任务：GAG-002、GAG-003

允许 Luna 处理批量包名、导入路径和文案替换；Flash 是本任务主模型，可处理确定性的 Fork、清理和脚手架，但最终构建与清理检查仍须完整执行。若同一验收项连续失败两次、上游结构与 v0.1.16 文档不符、出现 Tauri/Rust 构建错误或删除通用功能影响 ACP 基线，升级 Terra；若进一步涉及安全边界或跨层架构冲突，升级 Sol。

## 2. 目标与背景

从 `formulahendry/acp-ui` v0.1.16（固定 commit `cd9c3cb464a4b321bff652101953a64c07473e31`）建立可追溯基线，保留 MIT License 和 Git 历史，将产品重命名为 Grok ACP GUI，并移除 v1 明确不需要的 Web、移动端、通用 Agent 商店和遥测能力。完成后应是一个可启动、可构建、尚未实现产品功能的 Windows Tauri/Vue 壳。

## 3. 规范映射

- PRD：产品非目标、NFR-PRIVACY-001
- UI：主窗口基础尺寸，UI-ONBOARD-001 的空壳入口
- 技术：第 2、3、15 节
- Module：Composition root；本任务不实现五个深模块

## 4. 必读材料

1. 根 `AGENTS.md`
2. `docs/01-PRD.md` 第 2、6 节
3. `docs/03-TECHNICAL-DESIGN.md` 第 2、3、15 节
4. 上游 ACP UI README、LICENSE、package.json、Cargo.toml、Tauri config

## 5. 开始条件

- 目标目录为空或只有本计划交付的文档。
- Git、Node/npm、Rust stable-msvc、MSVC Build Tools 和 WebView2 可用。
- 能读取上游仓库；若网络不可用，停止在 Fork 步骤并保留文档，不创建伪造源码。

## 6. 实现范围

- Fork/clone v0.1.16，校验 HEAD 为固定 commit，并设置 `upstream`。
- 保留 MIT License 和第三方版权声明。
- 产品名、包名、Tauri identifier、窗口标题和内部命名改为 Grok ACP GUI。
- 保留 Windows desktop、Vue/Pinia、Tauri、ACP transport 基础、文件选择和本地持久化能力。
- 删除 Web build、移动端生成目录/脚本、WebSocket remote agent、通用 Agent Registry/Store、Application Insights 和默认第三方 Agent 配置。
- 创建技术方案约定的顶层目录和最小 composition root；不创建空的业务转发层。
- 设置基础脚本：typecheck、lint、test、build、tauri build。
- `.gitignore` 排除缓存、构建、日志、数据库、Worktree、恢复包和秘密。

## 7. 明确不实现

- 不实现 Grok 探测、ACP 会话、任务、图片或 Worktree。
- 不设计最终 UI 组件。
- 不添加 SQLite Schema。
- 不加入自动更新、签名、CI 发布或其他 Agent。

## 8. 允许修改

- 仓库根工程文件、LICENSE、README、package manifests、Tauri config。
- 上游已有 `src/`、`src-tauri/` 中与重命名、删除非范围和最小启动壳直接相关的文件。
- `.gitignore`、基础测试配置。

禁止修改 `docs/` 的需求决策；发现实际限制与文档冲突时报告，不擅自调整范围。

## 9. Interface 与数据影响

- 不定义最终 DesktopBridge；只保留最小 `bootstrap` 占位以支持应用启动，必须在 GAG-003 被替换。
- 不创建数据库或 Migration。
- 不向 Renderer 暴露 shell/fs/sql 任意权限。

## 10. 推荐实施顺序

1. 记录上游 tag/commit 和 License。
2. 安装锁定依赖并运行未修改基线构建，保存结果。
3. 重命名产品标识，逐处机械替换后重新构建。
4. 删除 Web/移动/遥测/Agent Store；每批删除后类型检查。
5. 收紧 Tauri capabilities，只保留启动壳所需权限。
6. 建立目标目录和脚本，不生成空业务 Module。
7. 更新 README：环境、开发命令、文档索引和当前未实现能力。

## 11. 异常与安全不变量

- 删除遥测后不得残留网络请求、API key、instrumentation key 或 opt-out 反逻辑。
- Tauri capabilities 不能用通配符提供任意 shell/path。
- 上游 License 不得移除或重写作者信息。
- 构建失败不能通过跳过 typecheck、降低 lint 或注释代码掩盖。

## 12. 自动化测试

- `npm ci`
- `npm run typecheck`
- `npm run lint`
- `npm run test`
- `npm run build`
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- 静态搜索不存在 Application Insights、remote WebSocket Agent、Android/iOS build script 和硬编码秘密。

## 13. 手工验收

1. Windows 启动开发窗口，标题和应用信息为 Grok ACP GUI。
2. 主窗口使用最小壳，无通用 Agent 选择器、遥测提示或 Web 连接入口。
3. DevTools 网络面板无应用自身遥测请求。
4. 关闭窗口后无残留应用进程。

## 14. Definition of Done

- 上游 commit 和 License 可追溯。
- Windows development build 与 production frontend build 通过。
- 非范围代码已删除而非仅隐藏。
- 没有产品功能的假实现、mock 或无责任 TODO。
- README 和根 AGENTS 指向四份主文档与任务目录。
- 按根 AGENTS 交付模板报告结果。

## 15. UI、外部交互与数据影响

- UI 只交付可启动的 App Shell 占位和 `UI-ONBOARD-001` 入口；loading、empty 与启动失败必须可见，不伪装任何业务数据。
- 允许的外部交互仅为获取上游源码、依赖安装和本地构建；不启动 Grok、Git 写操作或业务子进程。
- 本任务不创建 SQLite 文件或 Migration，不持久化项目、任务、会话和密钥。
- Fork 前记录上游 tag 与 commit；实际目录、脚本或 License 与预期不符时停止并提交差异，不由实施 AI自行改写产品范围。

## 16. 标准任务交付报告

报告必须包含：Task ID；实际主/辅助模型与 reasoning；升级记录；上游 URL、tag、commit 和 License；删除/保留能力清单；修改文件；构建与测试命令、退出码；遥测/秘密/权限扫描结果；手工截图；未完成项和风险。
