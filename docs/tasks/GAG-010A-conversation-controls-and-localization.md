# GAG-010A：对话配置、Slash Command、剪贴板与中文化功能归档

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-010A |
| 类型 | 对话配置 / ACP capability / 本地附件入口 / 本地化归档 |
| 难度 | D3 |
| 首选模型 | GPT-5.6 Terra |
| 备选模型 | GPT-5.6 Luna（仅机械性测试、文档与名称同步） |
| 推荐 reasoning effort | High |

本任务是既有成果的编号收敛，不重新设计产品或修改既有行为。GAG-011 继续且仅表示 Worktree 生命周期；GAG-012 继续且仅表示 Diff 与 Checkpoint。

## 2. 归档目标与范围

把提交区间 `93447e8` 至 `4a884d1` 中原先标为 GAG-010/GAG-011 的对话配置成果归档为 GAG-010A：

- 会话内模式选择：智能体、计划、问答；保存后在 `task.open` 恢复，并在下一 Turn 通过 ACP `session/set_mode` 生效。
- 模型和 reasoning 选择、校验、持久化及下一 Turn 传递。
- ACP Slash Commands 的发现、类型化事件、持久化回放和 Composer 菜单。
- 剪贴板图片粘贴，经受管 Artifact 导入而非 Renderer 任意路径访问。
- 可选任务标题和由首个有效句子派生标题。
- 已触达页面的全中文文案与相应回归覆盖。
- 已存在的模式—工作区策略联动仅作为会话配置与 fail-closed 行为归档；它**不**创建、列出、删除或恢复 Worktree。

非范围：GAG-011 Worktree 生命周期、GAG-012 Diff/Checkpoint、GAG-013 Squash、远程 Git 写入、Migration 重写、产品行为重构。

## 3. 需求与实现映射

| 需求/界面 | 既有实现 | 覆盖测试 |
|---|---|---|
| FR-TASK-001、UI-TASK-002：标题、模式、模型、reasoning、附件配置 | `src/features/task-center/CreateTaskDialog.vue`、`src/features/task-center/title.ts`、`src/bridge/types.ts`、`src-tauri/src/bridge/commands.rs` | `src-tauri/tests/gag_010a_conversation_controls.rs`、`tests/ui/gag-010a-chat-paste-model-i18n-slash.spec.ts` |
| FR-SESSION-003：动态模型、模式、reasoning、Slash Commands | `src/features/conversation/ConversationHeader.vue`、`conversation-store.ts`、`slash-commands.ts`、`src-tauri/src/adapters/grok_acp/interpreter.rs`、`src-tauri/src/modules/agent_runtime/{events,runtime}.rs` | `src-tauri/tests/gag_010a_conversation_controls.rs`、`tests/ui/gag-010a-mode-switch.spec.ts`、`tests/e2e/gag-010a-mode-switch.spec.ts` |
| FR-SESSION-001/004、UI-CONV-001：配置保存、重新打开和下一 Turn 生效 | `src/features/conversation/ConversationView.vue`、`conversation-facade.ts`、`src-tauri/src/bridge/dispatch.rs`、`src-tauri/src/modules/task_runtime/mailbox.rs` | 同上 Rust/模式 UI/E2E 测试 |
| FR-IMAGE-001、UI-CONV-001：剪贴板图片粘贴 | `src/features/conversation/Composer.vue`、`clipboard-images.ts`、`src-tauri/src/modules/artifacts/mod.rs` | `tests/ui/gag-010a-chat-paste-model-i18n-slash.spec.ts`、`tests/e2e/gag-010a-chat-paste-model-i18n-slash.spec.ts` |
| 中文界面与模式—策略会话配置 | `src/app/{AppShell,ShellView,UiKitFixture}.vue`、`src/features/conversation/mode-workspace.ts` | `tests/ui/gag-010a-workspace-linkage.spec.ts`、`tests/e2e/gag-010a-workspace-linkage.spec.ts` |

Bridge 契约保持一致：Renderer 仍只经 `src/bridge/` 调用 DesktopBridge；`session.configure`、`task.open` 与 `session.commands.updated` 的 Rust DTO、dispatch、Fake ACP 和 TypeScript 类型同步变更均已存在。本归档不新增 DTO、事件、状态机或 SQLite Migration。

## 4. 历史编号偏差记录

不改写已推送或已合并历史。下列历史提交的内容归入 GAG-010A，但提交信息保持原样：

| 历史提交 | 原编号 | 归档编号 | 内容 |
|---|---|---|---|
| `93447e8`、`80189f8`、`b6a6cd7`、`511021d` | GAG-010 | GAG-010A | 模型/reasoning、Slash、剪贴板、标题、中文化与测试 |
| `d7cc37e`、`32e6703`、`4a884d1` | GAG-011 | GAG-010A | 对话模式选择、持久化/恢复与模式测试 |
| `b3c94bc` | GAG-010（Squash merge） | GAG-010A（文档与测试名称归档） | 当前 `main` 中上述成果的合并提交；其中真实 Worktree 生命周期未实现 |

`GAG-011-worktree-lifecycle.md` 与 `GAG-012-diff-and-checkpoints.md` 未被修改。若未来确需改写上述历史提交信息，必须先取得用户的明确批准。

## 5. 当前完成状态（2026-08-09）

| 类别 | 状态 |
|---|---|
| 已合并到 `main` | GAG-001～GAG-010 的既有能力，以及本说明书映射的 GAG-010A 产品实现（位于 `b3c94bc`）。 |
| 当前分支已提交 | 无；`docs/GAG-010A-conversation-controls` 从 `b3c94bc` 创建，尚未创建本任务提交。 |
| 当前工作区未提交 | 本说明书、路线图/README 同步，以及 GAG-010A 对话配置测试文件和描述的重命名。 |
| 尚未实现 | GAG-011 Worktree 生命周期；GAG-012 Diff 与 Checkpoint；以及后续 GAG-013～GAG-016。 |

## 6. 验收与回归

- 不出现把模式选择新增称为 GAG-011 Worktree 生命周期的引用。
- 原 GAG-011/012 任务说明书保持原定义不变。
- 本说明书、路线图、README、受影响测试名和测试注释一致使用 GAG-010A。
- 运行 `rg` 编号审计、`npm run typecheck`、`npm run lint`、`npm run test`、`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml` 和 `git diff --check`。

## 7. 交付记录

- 实际模型：GPT-5.6 Terra，reasoning High。
- Interface/事件/DTO：无新增；仅记录既有 Rust/TypeScript 统一契约。
- SQLite Migration：无。
- 回滚方式：撤销本归档分支上的文档与测试重命名提交；不触碰已合并历史。
