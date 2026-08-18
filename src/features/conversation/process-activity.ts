import { isAskTool, isExploreTool } from "./tool-normalize";
import type {
  ProcessActivityEntry,
  ProcessActivityItem,
  TimelineItem,
  ToolItem,
} from "./types";

function isProcessEntry(item: TimelineItem): item is ProcessActivityEntry {
  if (item.kind === "thinking") return true;
  if (item.kind === "tool") return !isAskTool(item.tool);
  return item.kind === "activity" && item.activityKind !== "changes";
}

function readCount(tool: ToolItem): number {
  if (!isExploreTool(tool.tool)) return 0;
  const folded = /^已查看\s+(\d+)\s+项$/.exec(tool.tool.title.trim());
  return folded ? Number(folded[1]) : 1;
}

function buildGroup(
  entries: ProcessActivityEntry[],
  expandedIds: ReadonlySet<string>,
): ProcessActivityItem {
  const first = entries[0];
  const last = entries[entries.length - 1];
  const id = `process-${first.id}`;
  const tools = entries.filter((entry): entry is ToolItem => entry.kind === "tool");
  const failed = tools.filter((entry) => entry.tool.phase === "failed").length;
  const running = entries.some(
    (entry) =>
      (entry.kind === "tool" &&
        (entry.tool.phase === "running" || entry.tool.phase === "pending")) ||
      (entry.kind === "thinking" && entry.durationMs == null),
  );
  const startedAt = Date.parse(first.timestamp);
  const endedAt = Date.parse(last.timestamp);

  return {
    id,
    kind: "process",
    seq: last.seq,
    sessionId: first.sessionId,
    timestamp: first.timestamp,
    eventKey: first.eventKey,
    entries,
    expanded: expandedIds.has(id),
    phase: failed > 0 ? "attention" : running ? "running" : "completed",
    durationMs:
      Number.isFinite(startedAt) && Number.isFinite(endedAt)
        ? Math.max(0, endedAt - startedAt)
        : undefined,
    counts: {
      total: entries.length,
      thinking: entries.filter((entry) => entry.kind === "thinking").length,
      reads: tools.reduce((total, tool) => total + readCount(tool), 0),
      executes: tools.filter((tool) => !isExploreTool(tool.tool)).length,
      failed,
    },
  };
}

/**
 * Fold a continuous run of process events into one quiet presentation row.
 * Messages, decisions, errors, artifacts, and change whispers remain barriers.
 */
export function foldProcessActivities(
  items: readonly TimelineItem[],
  expandedIds: ReadonlySet<string> = new Set(),
): TimelineItem[] {
  const output: TimelineItem[] = [];
  let pending: ProcessActivityEntry[] = [];

  const flush = () => {
    if (pending.length === 1) output.push(pending[0]);
    else if (pending.length > 1) output.push(buildGroup(pending, expandedIds));
    pending = [];
  };

  for (const item of items) {
    if (isProcessEntry(item)) pending.push(item);
    else {
      flush();
      output.push(item);
    }
  }
  flush();
  return output;
}
