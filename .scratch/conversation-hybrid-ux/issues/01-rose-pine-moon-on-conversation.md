# 01 — 对话面挂上 Rose Pine Moon

**What to build:** Opening a conversation uses Rose Pine Moon (base, surface, iris, love, foam) instead of the gray Mocha field. Task center, review, and recovery may still look Mocha. No theme picker and no live sync with the Grok TUI.

**Blocked by:** None — can start immediately.

**Status:** implemented

**Parent:** `docs/tasks/GAG-021-conversation-hybrid-ux.md` · ADR-0003

- [x] Conversation canvas, task bar, composer, and rail read as Rose Pine Moon, not Mocha gray surfaces
- [x] Non-conversation pages still render with the existing Mocha tokens
- [x] New colors go through design tokens; contrast check still passes for the conversation surface
- [x] No DesktopBridge, ACP, or settings-theme API change
