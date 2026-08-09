# Grok ACP GUI

Grok ACP GUI 是 Windows 优先的本地桌面应用，计划通过结构化 Agent Client Protocol（ACP）连接 Grok Build，为开发者提供可观察、可审批、可恢复的编码工作流。

当前 `main` 已合并 GAG-001～GAG-010 的工程基线、设计系统、DesktopBridge、持久化、ACP runtime、恢复、任务中心、对话、Plan/权限和受管 Artifact 能力。对话模式、模型与 reasoning、Slash Commands、剪贴板图片、标题派生与中文化的归档编号为 GAG-010A；详见 [`docs/tasks/GAG-010A-conversation-controls-and-localization.md`](docs/tasks/GAG-010A-conversation-controls-and-localization.md)。

GAG-011 仍专指安全的 Worktree 生命周期，GAG-012 仍专指 Diff 与 Checkpoint；二者尚未因 GAG-010A 而实现或改变定义。

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

后续工作从 GAG-011 的 Worktree 生命周期开始，随后是 GAG-012 的 Diff 与 Checkpoint；任务顺序和依赖以路线图为准。
