import type { TimelineItem } from "./types";

/** Compact Grok-style process rows; larger cards keep a roomier estimate. */
export function estimateTimelineItemHeight(
  item: Pick<TimelineItem, "kind">,
  fallback = 80,
): number {
  switch (item.kind) {
    case "thinking":
    case "tool":
    case "process":
      return 36;
    case "activity":
    case "system":
      return 28;
    case "artifact":
      return 56;
    case "user":
      return 72;
    case "permission":
    case "plan":
      return 180;
    case "assistant":
      return 64;
    default:
      return fallback;
  }
}
