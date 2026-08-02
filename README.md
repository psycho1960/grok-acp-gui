# Grok ACP GUI

> Windows-first 桌面 GUI，用结构化 [Agent Client Protocol (ACP)](https://github.com/agentclientprotocol/agent-client-protocol) 连接 [Grok Build](https://docs.x.ai/docs/grok-build)，为不打算一直留在终端的开发者提供可观察、可审批、可恢复的编码工作流。

本仓库是 Grok ACP GUI 的**规格与工程基线**。当前 commit 只包含产品文档、`AGENTS.md` 开发规范以及 GAG-001 工程基线任务的实施授权。源代码将由 `docs/tasks/GAG-001-project-bootstrap.md` 在 fork 后续 commit 中落地。

---

## 项目定位

Grok ACP GUI **不重写 Grok 的 Agent 循环**，而是在其上提供：

- 项目、任务、图片、权限、Diff 和 Worktree 的统一桌面集成
- ACP 事件的流式时间线、原始选项回传、Plan mode fail-closed
- 默认隔离 Worktree 的并行任务；Squash 集成走临时 integration Worktree
- 进程崩溃、合并冲突和强制清理前都可验证恢复包
- 不保存 Grok Token / API Key；复用 Grok CLI 登录或进程继承环境变量

非目标（v1）：完整 IDE / LSP / 调试器、Web / 移动端、macOS / Linux 首发、通用 Agent 商店、自动 Push / 创建 PR / 删除远程分支、语音输入与独立文生图工作台。

---

## 文档索引

| 文件 | 作用 |
|---|---|
| [`AGENTS.md`](AGENTS.md) | 仓库级编码 Agent / Git / 安全 / 提交流程规范（必读） |
| [`docs/01-PRD.md`](docs/01-PRD.md) | 产品需求文档（FR / NFR / 成功标准 / 用户旅程） |
| [`docs/02-UI-UX-DESIGN.md`](docs/02-UI-UX-DESIGN.md) | UI/UX 设计规范（Catppuccin Mocha Token、信息架构、界面规格） |
| [`docs/03-TECHNICAL-DESIGN.md`](docs/03-TECHNICAL-DESIGN.md) | 技术方案（栈、目录、五个深模块、DesktopBridge 接口） |
| [`docs/04-AI-DEVELOPMENT-ROADMAP.md`](docs/04-AI-DEVELOPMENT-ROADMAP.md) | AI 开发路线图（任务顺序、依赖、模型策略、质量门禁） |
| [`docs/tasks/GAG-001-project-bootstrap.md`](docs/tasks/GAG-001-project-bootstrap.md) | 首项任务：fork acp-ui v0.1.16、清理、命名、目录骨架 |

`docs/adr/` 用于记录已接受的架构决策记录，遵循 `AGENTS.md` §1 的优先级。

---

## 技术栈（目标态）

- **桌面壳**：Tauri 2 + Windows 10/11 + WebView2
- **后端**：Rust stable-msvc、Tokio、Serde、Rusqlite
- **前端**：Vue 3、TypeScript、Pinia、Vite
- **协议**：ACP（@agentclientprotocol/sdk），下游以真实契约为准
- **Diff 预览**：Monaco Editor（仅预览/Diff，非编辑器）
- **上游基线**：fork [`formulahendry/acp-ui`](https://github.com/formulahendry/acp-ui) `v0.1.16`，固定 commit `cd9c3cb464a4b321bff652101953a64c07473e31`，保留 MIT License 与上游历史

依赖方向：`features → bridge → Rust bridge → modules → adapters`。

---

## 当前状态

| 维度 | 现状 |
|---|---|
| 源代码 | 未生成；等待 GAG-001 fork / 清理 / 骨架 |
| DesktopBridge | 仅占位 `bootstrap()`；正式契约见 GAG-003 |
| SQLite | 未引入；Migration 由 GAG-004 引入并不得回改 |
| CI / 打包 | 未配置；Windows MSI 由 GAG-016 提供 |
| 自动化测试 | 未引入；Vitest / Rust tests / Fake ACP 由 GAG-015 完成 |

**当前 commit 不包含任何应用代码**，仅作为规格冻结点。任何二次提交必须遵守 `AGENTS.md` 的分支、提交、安全与单任务边界。

---

## 开发流程

```text
feat/GAG-001-project-bootstrap  ─►  PR(Squash)  ─►  main
                              │
                              └─► npm run typecheck / lint / test
                              └─► cargo fmt / clippy / test
                              └─► npm run tauri build
```

详细门禁与交付模板见 `AGENTS.md` §10 与 §11。

---

## 参与

1. 阅读 [`AGENTS.md`](AGENTS.md) 与 [`docs/01-PRD.md`](docs/01-PRD.md)。
2. 在 [`docs/04-AI-DEVELOPMENT-ROADMAP.md`](docs/04-AI-DEVELOPMENT-ROADMAP.md) 中确认目标任务的依赖是否已合并。
3. 打开对应 `docs/tasks/GAG-*.md`，按推荐实施顺序编码。
4. 完成 PR 前预检，按统一交付模板填写报告。

---

## 许可证

仓库以 **MIT License** 发布，详见 [`LICENSE`](LICENSE)。代码尚未引入；fork 后的源码将保留上游 `formulahendry/acp-ui` 的 MIT 通知与第三方版权声明。