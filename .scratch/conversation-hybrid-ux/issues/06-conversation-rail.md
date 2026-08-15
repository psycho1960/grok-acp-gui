# 06 — Conversation rail

**What to build:** Beside the timeline there is at most one rail, switching between artifacts and workspace. It stays closed unless the task has artifacts or the workspace is attention-worthy. Agent-produced images are a compact chip in the stream and a gallery (preview/save/reveal) in the rail. The shell inspector does not add a second right column while conversation is open.

**Blocked by:** 01 — 对话面挂上 Rose Pine Moon; 02 — 混合时间线

**Status:** implemented

**Parent:** `docs/tasks/GAG-021-conversation-hybrid-ux.md`

- [x] Never two right columns during conversation
- [x] Rail closed when there are no artifacts and the workspace is a healthy isolation
- [x] Rail opens for artifacts, or for not-created / conflicted / external-awaiting-adoption / cleanup-recovery-pending
- [x] Timeline shows a compact artifact chip; gallery actions live in the rail
- [x] Clicking a user-bubble thumbnail opens the rail on that image
