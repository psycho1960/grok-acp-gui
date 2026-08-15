# 07 — 历史轮次

**What to build:** A clock on the task bar opens a list of turns. Each row is the first line of a sent user message plus relative time. Clicking a row scrolls to that user bubble. Queued follow-ups do not appear in the list.

**Blocked by:** 02 — 混合时间线

**Status:** implemented

**Parent:** `docs/tasks/GAG-021-conversation-hybrid-ux.md`

- [x] Task-bar clock opens the turn list
- [x] Rows are sent-user first lines only — no `#seq`, no queued items
- [x] Clicking a row scrolls to that turn’s user bubble
- [x] Empty conversation has an empty or disabled list, not fixture copy
