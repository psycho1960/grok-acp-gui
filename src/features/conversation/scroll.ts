// GAG-008: Auto-scroll and unread rules for the conversation timeline.

export interface ScrollAnchor {
  /** ScrollTop when user left the bottom. */
  scrollTop: number;
  /** Item eventKey near viewport top for restore. */
  anchorEventKey?: string;
  /** Whether stick-to-bottom is active. */
  stickToBottom: boolean;
  unreadCount: number;
}

export const BOTTOM_THRESHOLD_PX = 64;

export function createScrollAnchor(): ScrollAnchor {
  return {
    scrollTop: 0,
    stickToBottom: true,
    unreadCount: 0,
  };
}

export function isNearBottom(
  scrollTop: number,
  clientHeight: number,
  scrollHeight: number,
  threshold = BOTTOM_THRESHOLD_PX,
): boolean {
  return scrollTop + clientHeight >= scrollHeight - threshold;
}

/**
 * When new items arrive:
 * - if stickToBottom, keep following
 * - else increment unread
 */
export function onItemsAppended(
  anchor: ScrollAnchor,
  appendedCount: number,
): ScrollAnchor {
  if (appendedCount <= 0) return anchor;
  if (anchor.stickToBottom) {
    return { ...anchor, unreadCount: 0 };
  }
  return {
    ...anchor,
    unreadCount: anchor.unreadCount + appendedCount,
  };
}

export function onUserScroll(
  anchor: ScrollAnchor,
  scrollTop: number,
  clientHeight: number,
  scrollHeight: number,
): ScrollAnchor {
  const near = isNearBottom(scrollTop, clientHeight, scrollHeight);
  return {
    ...anchor,
    scrollTop,
    stickToBottom: near,
    unreadCount: near ? 0 : anchor.unreadCount,
  };
}

export function jumpToBottom(anchor: ScrollAnchor): ScrollAnchor {
  return {
    ...anchor,
    stickToBottom: true,
    unreadCount: 0,
  };
}

/** Per-session scroll memory (in-memory module map). */
const anchors = new Map<string, ScrollAnchor>();

export function loadScrollAnchor(sessionKey: string): ScrollAnchor {
  return anchors.get(sessionKey) ?? createScrollAnchor();
}

export function saveScrollAnchor(sessionKey: string, anchor: ScrollAnchor): void {
  anchors.set(sessionKey, { ...anchor });
}

export function clearScrollAnchor(sessionKey: string): void {
  anchors.delete(sessionKey);
}
