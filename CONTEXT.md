# Grok ACP GUI

Windows 桌面任务控制台：用结构化 ACP 运行 Grok Build，让开发者在一个窗口里观察、审批、审查和恢复编码任务。

## Language

**Conversation**:
The task-scoped surface whose spine is user and assistant messages, while tool calls, permission requests, and plans remain first-class work on the same timeline.
_Avoid_: Chat, chatbot, 聊天窗, log viewer

**Work card**:
A first-class, auditable timeline item for a tool call, permission request, or plan. It is not an attachment hanging off a message. Work cards stay full-width on the timeline. Collapsed: icon, title, status, one-line summary. Duration sits in the right cluster with copy and expand, not in the empty middle. Expanded: same header, then paths and ACP-safe detail in a scrollable mono block; copy and collapse are icons, not text links.
_Avoid_: Chat attachment, tool bubble, debug event, 展开/复制摘要 as visible text

**Message**:
A user or assistant utterance on the conversation timeline. The user is a right-aligned bubble. Images the user sent sit inside that bubble as 72×72 thumbnails; click opens the conversation rail. The assistant is left-aligned full-width prose with no card chrome. Work cards, whispers, and agent artifact chips stay left-aligned on the assistant edge. Messages carry no kind badge and no event sequence; relative time appears only after a gap.
_Avoid_: left-aligned user block, ▧ filename chips as the only image UI, #seq, avatar gutter, same card chrome as work cards

**Pending user message**:
A user bubble that has not been accepted yet. Same shape and alignment as a confirmed bubble, at reduced opacity, with a right-aligned whisper 「发送中」 or 「排队中」. It is not a work card and not an error.
_Avoid_: extra gray card, 发送中 as a full timeline row

**Follow-up while running**:
While a turn is running, the composer dock shows only Stop — never a second Send beside it. Enter queues the draft onto the bar above the composer. That bar's icons, in order, are edit, send now (interrupt this turn), and delete. Stop in the dock cancels the turn and keeps the draft. Queuing does not skip permission or plan cards on the later turn.
_Avoid_: two circular buttons in the running dock, Enter interrupts, text buttons on a dashed chat bubble

**Event sequence**:
The internal order of items in a conversation. It is not user-facing.
_Avoid_: #3, seq badge

**Session settings**:
Mode and workspace strategy are each a badge on the task bar. When they can change, the badge is a menu (chevron). When they cannot (running, cancelling, waiting permission, saving, sending), the badge is static: no chevron, no menu affordance, tooltip explains why. Model and reasoning live in the Composer dock. The task title appears once, on the conversation task bar; the shell breadcrumb is `project / 对话` without repeating the title.
_Avoid_: four header selects, 会话设置, disabled dropdown that still looks switchable, 对话：标题 in the breadcrumb

**Attempt**:
A numbered retry of the conversation session. The first attempt is not labeled.
_Avoid_: 第 1 次尝试 as default chrome

**Turn**:
One user-sent message plus the assistant reply and work cards that follow it, until the next user message. Users jump turns from a task-bar list of those user first lines, not by event sequence. Queued follow-ups are not turns and do not appear in that list.
_Avoid_: #seq as a turn number, queued items in the turn list

**Conversation status**:
Whether the conversation is idle, running, waiting, or failed. It lives on the task bar, not as a timeline row.
_Avoid_: 「已停止」 as a system message

**Change whisper**:
A quiet one-line note that workspace files changed. It is not a work card and not a reason to open the rail.
_Avoid_: changes activity card, `status: 快照完成`

**Composer**:
The conversation input for text and images. Inside one rounded dock: attach, a slash-command button, a compact model/reasoning selector, and Stop/Send. The slash button opens the same menu as typing `/`. The model/reasoning control shows a chevron only when it can change; while running it is a static label. Mode and workspace strategy do not live here.
_Avoid_: separate Stop and Send as the idle-only layout, dead hint text for slash commands, header Stop duplicate, a disabled dropdown that still looks switchable

**Thinking**:
A collapsed aside on the timeline. It reads 「思考中」 or 「已思考」 plus duration, and expands only to ACP-allowed content. It is weaker than a message or work card.
_Avoid_: Thinking…, streaming thought as body text

**Artifact**:
A user-facing image or result. On the timeline it is a compact chip at the moment it appeared; the conversation rail holds the gallery and save/reveal actions.
_Avoid_: 制品栏 as a second preview, raw cache path

**Work card skin**:
Visual treatment of a work card (spacing, type, chrome) that must not change option IDs, labels, default focus, or fail-closed rules. Permission and plan actions sit at the bottom-right of the card; button labels are centered.
_Avoid_: regrouping permission options, guessing option semantics

**Explored batch**:
Consecutive read-only tools folded into one work card. The user-facing title is Chinese, not 「Explored N items」.
_Avoid_: Explored N items

**Empty conversation**:
A conversation that has no messages or work cards yet. Title: 「把目标发给智能体」。Detail: 「下方输入；需要时用 / 看快捷指令，或点回形针加图」。Not a dashed empty-state card competing with the Composer.
_Avoid_: 「还没有消息」

**Hybrid conversation**:
The chosen presentation stance: messages read as a conversation; work cards stay independent and inspectable.
_Avoid_: Chat-first, console-first

**Grok Build theme**:
The conversation visual source is the current Grok Build TUI theme. This machine uses Rose Pine Moon (base `#232136`, surface `#2a273f`, iris `#c4a7e7`). This conflicts with the v1 Mocha lock in `docs/02-UI-UX-DESIGN.md`.
_Avoid_: Catppuccin Mocha as the conversation field, gray surface stacks

**Icon-and-label**:
Stable destinations and work kinds show a 16px (nav) or 14px (card) icon beside the text. Icons never replace a unique ACP option label.
_Avoid_: text-only nav rows, Unicode dingbats as status icons

**Conversation rail**:
The single optional column beside a conversation timeline. It shows artifacts or workspace, never both as two columns. It is open only when the task has artifacts, or the workspace is attention-worthy.
_Avoid_: Dual inspector, permanent artifact sidebar

**Attention-worthy workspace**:
A workspace that must interrupt the conversation: not yet created, conflicted, external and awaiting adoption, or cleanup/recovery awaiting confirmation. A healthy isolated workspace is not attention-worthy.
_Avoid_: Any worktree counts as needing the rail
