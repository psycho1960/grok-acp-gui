# Grok ACP GUI

Grok ACP GUI 是 Windows 优先的本地桌面应用，计划通过结构化 Agent Client Protocol（ACP）连接 Grok Build，为开发者提供可观察、可审批、可恢复的编码工作流。

当前分支完成 GAG-001 工程基线：保留上游 ACP UI 的源版本记录、Vue/Pinia、Tauri、Windows 桌面壳、文件夹选择和本地存储插件能力；暂不持久化项目、任务、会话或密钥，也不实现 Grok 探测、ACP 会话、任务、权限、图片、Diff、Worktree 或数据库。ACP SDK 依赖保留，实际传输接入由后续任务负责。

## 上游基线

- 上游仓库：<https://github.com/formulahendry/acp-ui>
- 发布标签：`v0.1.16`
- 固定 commit：`cd9c3cb464a4b321bff652101953a64c07473e31`
- 许可证：上游 MIT License 保留在 [`LICENSE`](LICENSE)
- Git remote：`upstream` 指向上游，`origin` 指向本项目仓库

首次合并前仍需完成一次性历史接入决策：当前 PR 以规格种子 `origin/main` 为父提交，固定上游 commit 保存在开发环境的 `upstream`/基线备份中，但不属于当前 PR HEAD 的祖先。该项与仓库的 squash-only 合并规则冲突，未在本分支擅自改写远程历史。

## 开发环境

- Windows 10/11、WebView2
- Node.js 与 npm
- Rust stable-msvc、Cargo、MSVC Build Tools

## 常用命令

```powershell
npm ci
npm run typecheck
npm run lint
npm run test
npm run build
npm run tauri build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## 文档

- [`AGENTS.md`](AGENTS.md)：仓库级开发、Git、安全和交付规范
- [`docs/01-PRD.md`](docs/01-PRD.md)：产品需求
- [`docs/02-UI-UX-DESIGN.md`](docs/02-UI-UX-DESIGN.md)：UI/UX 规范
- [`docs/03-TECHNICAL-DESIGN.md`](docs/03-TECHNICAL-DESIGN.md)：技术方案
- [`docs/04-AI-DEVELOPMENT-ROADMAP.md`](docs/04-AI-DEVELOPMENT-ROADMAP.md)：开发路线图
- [`docs/tasks/`](docs/tasks/)：GAG 任务说明书

后续任务会逐步替换当前 bootstrap 占位，并实现 DesktopBridge、Grok ACP runtime、任务中心、审查和 Worktree 生命周期。
