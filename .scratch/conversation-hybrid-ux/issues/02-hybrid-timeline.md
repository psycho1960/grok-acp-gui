# 02 — 混合时间线

**What to build:** The timeline reads as a hybrid conversation: the user's message is a right-aligned bubble (images as 72×72 thumbs inside it); the assistant is left-aligned prose without card chrome; tools, permissions, and plans stay full-width work cards. No kind badge, no event sequence. Thinking is 「思考中」/「已思考」. Read-only batches are titled 「已查看 N 项」. Work cards collapse by default; duration sits with copy/expand on the right. Heartbeats leave the timeline; file changes are a change whisper. Empty state uses the agreed welcome copy.

**Blocked by:** 01 — 对话面挂上 Rose Pine Moon

**Status:** implemented

**Parent:** `docs/tasks/GAG-021-conversation-hybrid-ux.md`

- [x] User messages sit on the right; assistant prose and work cards share the left edge
- [x] Production UI never shows `#seq` or a 用户/助手 kind badge
- [x] Relative time appears only after a gap; exact time is hover-only
- [x] Thinking is a Chinese collapsed aside; expands only to ACP-allowed content
- [x] Explored-batch title is 「已查看 N 项」
- [x] Work-card duration is in the right cluster; copy/collapse are icons
- [x] Snapshot/heartbeat/「已停止」 are not timeline rows; file changes are a one-line whisper
- [x] Empty conversation shows 「把目标发给智能体」 and the agreed hint
- [x] Tests go through the conversation page / fake-bridge seam
