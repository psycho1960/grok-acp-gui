// GAG-008: Auto-scroll and unread rules for the conversation timeline.

export interface ScrollAnchor {
  /** ScrollTop when user left the bottom. */
  scrollTop: number;
  /** Item eventKey near viewport top for restore. */
  anchorEventKey?: string;
  /** Pixel offset inside the anchored item. */
  anchorOffsetPx?: number;
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

const SCROLL_STORAGE_PREFIX = "gag008:scroll:";

function storage(): Storage | null {
  try {
    if (typeof window === "undefined") return null;
    return window.localStorage;
  } catch {
    return null;
  }
}

function storageKey(sessionKey: string): string {
  return `${SCROLL_STORAGE_PREFIX}${sessionKey}`;
}

function parseStoredAnchor(raw: string | null): ScrollAnchor | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Record<string, unknown>;
    if (
      !Number.isFinite(value.scrollTop) ||
      Number(value.scrollTop) < 0 ||
      typeof value.stickToBottom !== "boolean" ||
      !Number.isSafeInteger(value.unreadCount) ||
      Number(value.unreadCount) < 0
    ) {
      return null;
    }
    const eventKey =
      typeof value.anchorEventKey === "string" && value.anchorEventKey.length <= 512
        ? value.anchorEventKey
        : undefined;
    const anchorOffsetPx =
      Number.isFinite(value.anchorOffsetPx) && Number(value.anchorOffsetPx) >= 0
        ? Number(value.anchorOffsetPx)
        : undefined;
    return {
      scrollTop: Number(value.scrollTop),
      ...(eventKey ? { anchorEventKey: eventKey } : {}),
      ...(anchorOffsetPx != null ? { anchorOffsetPx } : {}),
      stickToBottom: value.stickToBottom,
      unreadCount: Number(value.unreadCount),
    };
  } catch {
    return null;
  }
}

/** Per-session scroll memory, persisted so refresh/restart restores position. */
const anchors = new Map<string, ScrollAnchor>();

export function loadScrollAnchor(sessionKey: string): ScrollAnchor {
  const inMemory = anchors.get(sessionKey);
  if (inMemory) return { ...inMemory };
  const persisted = parseStoredAnchor(storage()?.getItem(storageKey(sessionKey)) ?? null);
  if (persisted) {
    anchors.set(sessionKey, persisted);
    return { ...persisted };
  }
  return createScrollAnchor();
}

export function saveScrollAnchor(sessionKey: string, anchor: ScrollAnchor): void {
  const safeAnchor = { ...anchor };
  anchors.set(sessionKey, safeAnchor);
  try {
    storage()?.setItem(storageKey(sessionKey), JSON.stringify(safeAnchor));
  } catch {
    // Storage may be unavailable or full; in-memory switching still works.
  }
}

export function clearScrollAnchor(sessionKey: string): void {
  anchors.delete(sessionKey);
  try {
    storage()?.removeItem(storageKey(sessionKey));
  } catch {
    // ignore unavailable storage
  }
}
