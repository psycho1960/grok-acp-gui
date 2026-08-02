# Grok ACP GUI

Grok ACP GUI 是 Windows 优先的本地桌面应用，计划通过结构化 Agent Client Protocol（ACP）连接 Grok Build，为开发者提供可观察、可审批、可恢复的编码工作流。

当前分支完成 GAG-001 工程基线：保留上游 ACP UI 的源版本记录、Vue/Pinia、Tauri、Windows 桌面壳、文件夹选择、本地存储插件能力和 newline-framed stdio transport 基础；暂不持久化项目、任务、会话或密钥，不创建或启动业务子进程，也不实现 Grok 探测、ACP 会话、任务、权限、图片、Diff、Worktree 或数据库。ACP stream wiring 由后续任务接入。

## 上游基线

- 上游仓库：<https://github.com/formulahendry/acp-ui>
- 发布标签：`v0.1.16`
- 固定 commit：`cd9c3cb464a4b321bff652101953a64c07473e31`
- 许可证：上游 MIT License 保留在 [`LICENSE`](LICENSE)
- Git remote：`upstream` 指向上游，`origin` 指向本项目仓库

根据 [`ADR-0001`](docs/adr/ADR-0001-upstream-provenance-without-shared-ancestry.md)，本仓库以规格种子 `origin/main` 作为产品历史根，通过上游 URL、tag、完整 commit、License、版权声明和已校验源码快照记录来源；固定上游 commit 不要求属于 PR HEAD 或 `origin/main` 的 Git 祖先链。`main` 继续只接受 Squash Merge。

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
- [`docs/adr/`](docs/adr/)：已接受的架构与仓库决策

后续任务会逐步替换当前 bootstrap 占位，并实现 DesktopBridge、Grok ACP runtime、任务中心、审查和 Worktree 生命周期。
