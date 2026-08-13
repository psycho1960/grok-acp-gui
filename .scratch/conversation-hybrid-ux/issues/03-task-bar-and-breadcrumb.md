# 03 — 任务条与面包屑

**What to build:** The conversation chrome has one title on the task bar and a shell breadcrumb `project / 对话`. Back is a chevron to the task center. Mode and workspace are badges: chevron menus when idle; static labels (no menu affordance) while running, cancelling, waiting permission, saving, or sending. First attempt is unlabeled.

**Blocked by:** 01 — 对话面挂上 Rose Pine Moon

**Status:** implemented

**Parent:** `docs/tasks/GAG-021-conversation-hybrid-ux.md`

- [x] Task title appears once, on the task bar — never as `对话：标题` in the breadcrumb
- [x] Breadcrumb is `project / 对话`
- [x] Back is a 32×32 ghost chevron, not a raw `←`
- [x] Idle: `智能体 ▾` and workspace badge open menus; mode switch still applies default workspace
- [x] Locked states: no chevron, no menu, tooltip explains why
- [x] 「第 1 次尝试」 is not shown on the first attempt
- [x] Conversation status is a single control on the task bar
