// GAG-008: Normalize untyped toolCall payloads into ToolCallView.
// Frontend never "decrypts" redacted fields; only surfaces backend summaries.

import type { ToolCallView, ToolPhase } from "./types";

const TERMINAL: ReadonlySet<ToolPhase> = new Set([
  "completed",
  "failed",
  "cancelled",
]);

const PHASE_RANK: Record<ToolPhase, number> = {
  pending: 0,
  running: 1,
  completed: 2,
  failed: 2,
  cancelled: 2,
};

export function isTerminalPhase(phase: ToolPhase): boolean {
  return TERMINAL.has(phase);
}

/** Late updates must not downgrade a terminal tool back to running/pending. */
export function mergeToolPhase(
  current: ToolPhase,
  incoming: ToolPhase,
): ToolPhase {
  if (isTerminalPhase(current) && !isTerminalPhase(incoming)) {
    return current;
  }
  if (PHASE_RANK[incoming] < PHASE_RANK[current] && isTerminalPhase(current)) {
    return current;
  }
  return incoming;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((v): v is string => typeof v === "string");
}

function parsePhase(raw: unknown): ToolPhase {
  if (typeof raw !== "string") return "running";
  const n = raw.toLowerCase().replace(/-/g, "_");
  if (n === "pending" || n === "queued") return "pending";
  if (n === "running" || n === "in_progress" || n === "inprogress") return "running";
  if (n === "completed" || n === "success" || n === "ok" || n === "done")
    return "completed";
  if (n === "failed" || n === "error") return "failed";
  if (n === "cancelled" || n === "canceled" || n === "aborted") return "cancelled";
  return "running";
}

function redactedField(
  summary: string | undefined,
  redactedFlag: unknown,
  fallback: string,
): { summary: string; redacted: boolean } {
  const flag =
    redactedFlag === true ||
    redactedFlag === "true" ||
    summary === "[redacted]";
  return {
    summary: summary && summary.length > 0 ? summary : fallback,
    redacted: flag || !summary,
  };
}

/**
 * Normalize ACP / bridge toolCall JSON into a safe ToolCallView.
 * Accepts ToolCallSummary-shaped objects and richer update payloads.
 */
export function normalizeToolCall(
  raw: unknown,
  fallbackId?: string,
): ToolCallView | null {
  const obj = asRecord(raw);
  if (!obj) return null;

  const toolCallId =
    asString(obj.toolCallId) ??
    asString(obj.tool_call_id) ??
    asString(obj.id) ??
    fallbackId;
  if (!toolCallId) return null;

  const title =
    asString(obj.title) ??
    asString(obj.name) ??
    asString(obj.toolName) ??
    "Tool";
  const kind = asString(obj.kind) ?? asString(obj.category) ?? "unknown";
  const phase = parsePhase(obj.status ?? obj.phase ?? obj.state);
  const startedAt = asString(obj.startedAt) ?? asString(obj.started_at);
  const endedAt = asString(obj.endedAt) ?? asString(obj.ended_at);
  const durationMs =
    asNumber(obj.durationMs) ??
    asNumber(obj.duration_ms) ??
    (startedAt && endedAt
      ? Math.max(0, Date.parse(endedAt) - Date.parse(startedAt))
      : undefined);

  const locations = asStringArray(obj.locations ?? obj.paths);

  const inputRaw =
    asString(obj.inputSummary) ??
    asString(obj.input_summary) ??
    asString(obj.commandSummary) ??
    asString(obj.command_summary);
  const resultRaw =
    asString(obj.resultSummary) ??
    asString(obj.result_summary) ??
    asString(obj.outputSummary) ??
    asString(obj.output_summary);

  const inputRedacted = obj.inputRedacted ?? obj.input_redacted ?? obj.redacted;
  const resultRedacted =
    obj.resultRedacted ?? obj.result_redacted ?? obj.redacted;

  const input = redactedField(inputRaw, inputRedacted, "参数已隐藏");
  const result = redactedField(
    resultRaw,
    resultRedacted,
    phase === "completed" || phase === "failed" ? "结果摘要不可用" : "…",
  );

  // Force redaction when backend marks the whole payload.
  if (obj.redacted === true) {
    input.redacted = true;
    result.redacted = true;
  }

  let detailsSafe: string | undefined;
  const details = asString(obj.detailsSafe) ?? asString(obj.details_safe);
  if (details && obj.redacted !== true) {
    detailsSafe = details.slice(0, 2000);
  }

  const exitCode = asNumber(obj.exitCode) ?? asNumber(obj.exit_code);
  const foldGroup = asString(obj.foldGroup) ?? asString(obj.fold_group);

  return {
    toolCallId,
    title,
    kind,
    phase,
    startedAt,
    endedAt,
    durationMs,
    locations,
    input,
    result,
    exitCode,
    foldGroup,
    detailsSafe,
  };
}

export function mergeToolCall(
  existing: ToolCallView,
  incoming: ToolCallView,
): ToolCallView {
  const phase = mergeToolPhase(existing.phase, incoming.phase);
  const startedAt = existing.startedAt ?? incoming.startedAt;
  const endedAt =
    phase !== "running" && phase !== "pending"
      ? (incoming.endedAt ?? existing.endedAt)
      : existing.endedAt;
  let durationMs = incoming.durationMs ?? existing.durationMs;
  if (durationMs == null && startedAt && endedAt) {
    durationMs = Math.max(0, Date.parse(endedAt) - Date.parse(startedAt));
  }

  return {
    toolCallId: existing.toolCallId,
    title: incoming.title !== "Tool" ? incoming.title : existing.title,
    kind: incoming.kind !== "unknown" ? incoming.kind : existing.kind,
    phase,
    startedAt,
    endedAt,
    durationMs,
    locations:
      incoming.locations.length > 0 ? incoming.locations : existing.locations,
    input:
      incoming.input.summary !== "参数已隐藏" || !incoming.input.redacted
        ? incoming.input
        : existing.input,
    result:
      incoming.result.summary !== "…" &&
      incoming.result.summary !== "结果摘要不可用"
        ? incoming.result
        : existing.result,
    exitCode: incoming.exitCode ?? existing.exitCode,
    foldGroup: incoming.foldGroup ?? existing.foldGroup,
    detailsSafe: incoming.detailsSafe ?? existing.detailsSafe,
  };
}

export function formatDuration(ms: number | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(s < 10 ? 1 : 0)}s`;
  const m = Math.floor(s / 60);
  const rem = Math.round(s % 60);
  return `${m}m ${rem}s`;
}
