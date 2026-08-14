# GAG-021 — 对话混合时间线体验

## 元数据

| 项 | 值 |
|----|-----|
| 分支 | `feat/GAG-021-conversation-hybrid-ux` |
| 标签 | `implemented` |
| 依据 | 本规格；根 `CONTEXT.md`；`docs/adr/ADR-0003-conversation-uses-grok-build-theme.md` |
| 依赖 | GAG-008、GAG-010A/B、GAG-017–020 |
| 首选模型 | 能完成前端实现的模型；安全相关权限/计划只换皮，不改语义 |
| 测试缝 | **对话页（用户可见面）**，经现有假 Bridge / Conversation Fixture。不新开缝。 |

---

## Problem Statement

对话页现在像协议日志：每条都有种类徽章、秒级时间和事件序号；用户和助手都是通栏灰卡片；头上四个设置下拉压过标题；制品栏和壳层检查器并排占宽；运行中无法自然跟进。开发者看不清「谁在说话、工作做到哪、下一步点哪里」，也不想像在用聊天机器人或缩水 IDE。

## Solution

把对话做成 **Hybrid conversation**：消息读起来像一轮工作记录，工具、权限和计划仍是独立 **Work card**。一条可选 **Conversation rail**；任务条只报状态和可改的身份；**Composer** 是圆角输入坞；运行中用坞上排队条跟进。色源为当前 Grok Build 主题 Rose Pine Moon（见 ADR-0003）。

## User Stories

1. As a developer, I want the conversation to read as a work record rather than a protocol dump, so that I can follow what the agent did without scanning debug chrome.
2. As a developer, I want my messages on the right and the assistant on the left, so that I can tell who spoke at a glance.
3. As a developer, I want images I sent to appear as 72×72 thumbnails inside my bubble, so that I can see what I attached without opening a file chip.
4. As a developer, I want clicking a sent thumbnail to open the conversation rail, so that I can preview, save, or reveal the image.
5. As a developer, I want tool calls, permission requests, and plans to stay full-width work cards, so that I can audit work without hunting inside a chat bubble.
6. As a developer, I want consecutive read-only tools folded as 「已查看 N 项」, so that exploration does not flood the timeline.
7. As a developer, I want a collapsed work card to show icon, title, status, one-line summary, and duration clustered on the right, so that I can scan quickly.
8. As a developer, I want to expand a work card to see paths and ACP-safe detail, so that I can audit without leaving the timeline.
9. As a developer, I want copy and collapse to be icons, so that the header stays quiet.
10. As a developer, I want thinking to appear as 「思考中」 or 「已思考」 plus duration, so that I know the agent is working without reading a thought dump.
11. As a developer, I want thinking to expand only to ACP-allowed content, so that hidden reasoning stays hidden.
12. As a developer, I want no kind badge and no event sequence on messages, so that the timeline does not look like a log.
13. As a developer, I want relative time only after a gap, so that nearby messages are not stamped twice.
14. As a developer, I want exact time on hover, so that I can still pinpoint a moment.
15. As a developer, I want heartbeat, snapshot, and 「已停止」 off the timeline, so that status is not a fake message.
16. As a developer, I want file-change notices as a change whisper, so that I know the workspace moved without opening a card.
17. As a developer, I want errors to stay on the timeline, so that failures remain auditable.
18. As a developer, I want the task title only on the task bar, so that I am not reading the same name twice.
19. As a developer, I want the shell breadcrumb to read `project / 对话`, so that I know the page without repeating the task title.
20. As a developer, I want a back chevron to the task center, so that I can leave the conversation without hunting the breadcrumb.
21. As a developer, I want one conversation status on the task bar, so that I do not see a duplicate idle badge.
22. As a developer, I want mode and workspace as badges, so that I can see the session identity without four selects.
23. As a developer, I want those badges to become menus with a chevron when idle, so that I can switch 问答 / 智能体 / 计划 and workspace strategy.
24. As a developer, I want those badges to lose the chevron while running, cancelling, waiting permission, saving, or sending, so that I am not invited to change rules mid-turn.
25. As a developer, I want a tooltip when a locked badge is hovered, so that I know why I cannot switch.
26. As a developer, I want changing mode while idle to apply the default workspace for that mode, so that ask stays writable-current and agent/plan stay isolated unless I override.
27. As a developer, I want readonly workspace never auto-selected, so that I only get it by choosing it.
28. As a developer, I want switching to an unready isolated workspace to refuse falling back to the original directory, so that writes cannot silently hit my working tree.
29. As a developer, I want model and reasoning in one composer control, so that I change them where I send.
30. As a developer, I want that control to drop its chevron while it cannot change, so that it matches locked badges.
31. As a developer, I want attach, slash-command, model/reasoning, and a single circular send/stop inside one dock, so that the composer feels like Grok/Codex, not a form.
32. As a developer, I want the slash button to open the same menu as typing `/`, so that mouse and keyboard share one command list.
33. As a developer, I want Enter to send when idle and the dock has content, so that sending stays familiar.
34. As a developer, I want Shift+Enter to newline, so that I can write lists.
35. As a developer, I want the circular control to be Send when idle and Stop when running, so that one seat has one meaning.
36. As a developer, I want no second Send beside Stop while running, so that interrupt is not duplicated.
37. As a developer, I want Enter while running to queue my draft on a bar above the composer, so that I can follow up without stopping the turn.
38. As a developer, I want that bar’s icons in order: edit, send now, delete, so that I can change, interrupt-send, or drop a queued item.
39. As a developer, I want edit to put the queued text back in the composer, so that I can revise before it goes out.
40. As a developer, I want send-now on the bar to interrupt the current turn and send that queued item, so that I can barge in like Codex.
41. As a developer, I want delete on the bar to drop the queued item, so that it never sends.
42. As a developer, I want Stop to cancel the turn and keep the composer draft, so that stop never means send.
43. As a developer, I want queued follow-ups to still hit permission and plan cards, so that queueing is not a security bypass.
44. As a developer, I want queued items not listed as turns, so that history only contains what landed on the timeline.
45. As a developer, I want a clock on the task bar that lists each turn by my first line, so that I can jump without scrolling the whole log.
46. As a developer, I want clicking a turn to scroll to my bubble for that turn, so that I land on the start of the work.
47. As a developer, I want the conversation rail to show either artifacts or workspace, so that I never get two right columns.
48. As a developer, I want the rail closed unless there are artifacts or the workspace is attention-worthy, so that the timeline can breathe.
49. As a developer, I want a healthy isolated workspace not to force the rail open, so that Agent/Plan tasks stay wide.
50. As a developer, I want agent-produced artifacts as a compact chip on the timeline and a gallery in the rail, so that cause stays in the stream and actions stay in the rail.
51. As a developer, I want permission and plan actions at the bottom-right with centered labels, so that decisions sit where I finish reading the card.
52. As a developer, I want permission option IDs, labels, default focus, and fail-closed rules unchanged, so that a prettier card cannot approve the wrong thing.
53. As a developer, I want nav rows and work kinds to show icon and label, so that destinations are scannable.
54. As a developer, I want ACP option labels never replaced by guessed icons, so that permission meaning stays the agent’s.
55. As a developer, I want an empty conversation to say 「把目标发给智能体」 with a short hint about `/` and attach, so that I know the next action is the dock.
56. As a developer, I want the first attempt unlabeled, so that a healthy session does not say 「第 1 次尝试」.
57. As a developer, I want Rose Pine Moon on the conversation surface, so that it matches the Grok Build TUI I already use.
58. As a developer, I want missed image cache to show a placeholder and explanation, so that I never see a broken-image icon.

## Implementation Decisions

- Renderer-only. No DesktopBridge, ACP, Git, SQLite Migration, or permission/plan semantic changes.
- Presentation stance is Hybrid conversation. Work cards stay first-class; messages are not chat attachments.
- One Conversation rail inside the conversation surface. When conversation is open, the shell inspector must not add a second right column. Collapse the rail unless artifacts exist or the workspace is attention-worthy (not yet created, conflicted, external awaiting adoption, cleanup/recovery awaiting confirmation).
- Timeline items share one left edge except the user bubble, which is right-aligned. No avatar gutter.
- Event sequence remains internal. Production UI never shows `#seq`.
- Conversation status lives on the task bar. Heartbeats, snapshots, and stopped-as-system-row leave the timeline. File changes are a change whisper.
- Session settings: mode and workspace strategy are task-bar badges; model and reasoning are one composer control. Chevron only when the control can change.
- Mode switch while idle still applies `ask → direct`, `agent|plan → worktree`. Readonly is user-chosen only. Unready worktree does not fall back to the original directory.
- Composer is one rounded dock. Slash button opens the same menu as `/`.
- Follow-up while running: dock shows only Stop; Enter queues onto the bar above the dock; bar icons are edit, send now (interrupt), delete. Stop keeps the draft. Queue is not a permission bypass.
- Turns: task-bar clock lists sent user first lines only. Queued items are not turns.
- Work card expand/collapse stays on the existing timeline item toggle. Duration clusters with copy and expand on the right.
- Explored batch title is Chinese 「已查看 N 项」.
- Theme: Rose Pine Moon on the conversation surface per ADR-0003. Other pages may remain Mocha this slice.
- Empty conversation copy is fixed: title 「把目标发给智能体」; detail 「下方输入；需要时用 / 看快捷指令，或点回形针加图」.
- Test seam is the conversation page with the existing fake bridge / fixture. Do not add a new module boundary for this work.
- Queue and interrupt are observable through that page. If the current send path cannot queue while `canSend` is false, extend the conversation store behind the same page seam without a new public Bridge command unless the backend already requires one. Prefer client-side queue flushed with the existing send turn after the current turn ends; interrupt uses the existing cancel-then-send path.
- Schematic reference (not shipped): `.scratch/conversation-hybrid-mockup.html`.

## Testing Decisions

- Test external, visible behavior on the conversation page. Do not assert CSS class names, virtual-list row heights, or reducer internals unless they are the only way to observe a user-facing rule.
- Good tests: given a fixture snapshot, the page shows or hides `#seq`, chevrons, the queue bar, the rail, turn-list rows, permission default focus, and Chinese explored-batch title.
- Modules under test: the conversation page and its store, through the existing fixture/fake-bridge seam used by `gag-008` and `gag-010a/b`.
- Prior art: `tests/ui/gag-008-components.spec.ts`, `gag-008-store.spec.ts`, `gag-010a-chat-paste-model-i18n-slash.spec.ts`, `gag-010b-workspace-linkage.spec.ts`, `gag-020-phase3.spec.ts`.
- Add focused UI tests for: locked badges without chevron while running; queue bar icon order; turn list excludes queued items; rail closed when workspace is healthy and there are no artifacts; send-now interrupts then sends; Stop does not send.
- Keep existing permission fail-closed and option-ID tests green. Visual restyle must not change those assertions.
- `npm run typecheck`, `npm run lint`, and the conversation-related `test:ui` / `test:node` suites are required. Do not invent ACP/Git/E2E passes.

## Out of Scope

- Review page, Monaco Diff, checkpoints, squash integration.
- Task-center 「click card opens conversation」 and left-nav information architecture beyond icon+label already present.
- Light theme, per-task rail pin memory, 「显示思考」 setting.
- Ctrl+F in conversation, Alt+↑/↓ turn keys.
- Runtime sync with Grok TUI `/theme`.
- Changing permission option IDs, labels, default focus, or fail-closed.
- DesktopBridge contract changes except if interrupt/queue cannot be done with existing cancel and send (must be called out before expanding).
- Chat-first bubbles for work cards.
- Minimum window height change.

## Further Notes

- Glossary: root `CONTEXT.md`. Theme conflict recorded in ADR-0003.
- Implementation must not parse Grok TUI visual text; theme copy is static Rose Pine Moon tokens in the design system, not scraped from the TUI.
- Suggested next after merge: `/to-tickets` if this spec is split; otherwise implement on `feat/GAG-021-conversation-hybrid-ux`.
