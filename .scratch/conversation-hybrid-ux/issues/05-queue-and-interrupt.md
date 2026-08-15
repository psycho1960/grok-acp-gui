# 05 — 运行中排队与打断

**What to build:** While a turn is running, Enter parks the draft on a bar above the composer. The bar’s icons are edit, send now, delete — in that order. Send now interrupts the current turn and sends that item. Delete drops it. Edit puts the text back in the dock. Stop in the dock cancels the turn and keeps the current draft. Queued items never skip permission or plan cards.

**Blocked by:** 04 — Composer 坞

**Status:** implemented

**Parent:** `docs/tasks/GAG-021-conversation-hybrid-ux.md`

- [x] Enter while running queues onto the bar; it does not interrupt
- [x] Icon order is edit → send now → delete
- [x] Send now interrupts then sends that queued item
- [x] Stop does not send; draft in the dock remains
- [x] Edit restores text to the composer and removes that bar row
- [x] A later queued turn still shows permission/plan cards with original option IDs and safe default focus
- [x] Observable through the conversation page seam (client queue + existing cancel/send unless a Bridge gap is documented first)
