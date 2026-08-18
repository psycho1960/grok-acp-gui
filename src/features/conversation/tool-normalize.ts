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
  const hasSummary = Boolean(summary && summary.length > 0);
  const flag =
    summary === "[redacted]" ||
    (hasSummary && (redactedFlag === true || redactedFlag === "true"));
  return {
    summary: flag ? "[redacted]" : summary && summary.length > 0 ? summary : fallback,
    redacted: flag,
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

  // Locations are already a backend-sanitized, user-visible part of the tool
  // event. Older ACP updates omitted rawInput while still marking that missing
  // field redacted; use the known targets instead of showing a blanket marker.
  const visibleInput =
    inputRaw ?? (locations.length > 0 ? locations.join(", ") : undefined);
  const input = redactedField(
    visibleInput,
    inputRaw == null && locations.length > 0 ? false : inputRedacted,
    "参数未提供",
  );
  const result = redactedField(
    resultRaw,
    resultRedacted,
    phase === "completed" || phase === "failed" ? "结果摘要不可用" : "…",
  );

  // A payload-level marker may accompany a sparse lifecycle update. Only
  // redact fields that are actually present; absence means “not provided”,
  // not “sensitive value hidden”.
  if (obj.redacted === true) {
    if (inputRaw) {
      input.summary = "[redacted]";
      input.redacted = true;
    }
    if (resultRaw) {
      result.summary = "[redacted]";
      result.redacted = true;
    }
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
      incoming.input.summary !== "参数已隐藏" &&
      incoming.input.summary !== "参数未提供" &&
      incoming.input.summary !== "[redacted]"
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

const HIDDEN_SUMMARIES = new Set(["参数已隐藏", "…", "结果摘要不可用", "[redacted]"]);

const EXPLORE_KINDS = new Set(["read", "search", "explore", "explore_batch"]);

const EXPLORE_TITLES = new Set([
  "read_file",
  "read",
  "list_dir",
  "ls",
  "glob",
  "grep",
  "search",
  "codebase_search",
  "find",
]);

/** Read-only Grok Build tools that should collapse into one explore batch. */
export function isExploreTool(tool: Pick<ToolCallView, "kind" | "title" | "foldGroup">): boolean {
  if (tool.foldGroup === "explore") return true;
  if (EXPLORE_KINDS.has(tool.kind.toLowerCase())) return true;
  return EXPLORE_TITLES.has(tool.title.trim().toLowerCase());
}

function looksLikeJsonDump(value: string): boolean {
  const trimmed = value.trim();
  return (trimmed.startsWith("{") || trimmed.startsWith("[")) && trimmed.length > 24;
}
function displayStructuredValue(value: unknown, indent = ""): string {
  if (typeof value === "string") return value;
  if (value == null || typeof value !== "object") return String(value);
  if (Array.isArray(value)) {
    return value
      .map((entry) => `${indent}- ${displayStructuredValue(entry, `${indent}  `)}`)
      .join("\n");
  }

  return Object.entries(value as Record<string, unknown>)
    .map(([key, entry]) => {
      const rendered = displayStructuredValue(entry, `${indent}  `);
      return rendered.includes("\n")
        ? `${indent}${key}:\n${rendered}`
        : `${indent}${key}: ${rendered}`;
    })
    .join("\n");
}

/**
 * Render ACP's JSON-encoded raw input/output as readable text. JSON decoding
 * restores embedded newlines so they respect the tool card's `pre-wrap` CSS.
 */
export function displayToolSummary(summary: string): string {
  try {
    return displayStructuredValue(JSON.parse(summary));
  } catch {
    return summary;
  }
}

export function shortenPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const parts = normalized.split("/").filter(Boolean);
  if (parts.length <= 2) return parts.join("/") || path;
  return parts.slice(-2).join("/");
}

/**
 * One-line process target for a collapsed Grok-style tool row.
 * Prefers a short path; never dumps raw JSON input.
 */
export function collapsedToolSummary(tool: ToolCallView): string {
  if (tool.kind === "explore_batch" && tool.result.summary && !tool.result.redacted) {
    return tool.result.summary;
  }
  if (tool.locations[0]) return shortenPath(tool.locations[0]);
  for (const candidate of [tool.result.summary, tool.input.summary]) {
    if (!candidate || HIDDEN_SUMMARIES.has(candidate) || looksLikeJsonDump(candidate)) {
      continue;
    }
    return candidate.split(/\r?\n/, 1)[0]?.trim() ?? "";
  }
  return "";
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

export interface AgentQuestionChoice {
  label: string;
  value: string;
}

export interface AgentQuestion {
  prompt: string;
  choices: AgentQuestionChoice[];
}

/** Grok currently exposes human clarification as an Ask tool call. */
export function isAskTool(
  tool: Pick<ToolCallView, "kind" | "title">,
): boolean {
  const kind = tool.kind.trim().toLowerCase();
  return (
    kind === "ask" ||
    kind === "question" ||
    kind === "ask_user" ||
    /^ask\s*[:：]/i.test(tool.title.trim())
  );
}

function questionChoice(value: unknown): AgentQuestionChoice | null {
  if (typeof value === "string" && value.trim()) {
    return { label: value.trim(), value: value.trim() };
  }
  const record = asRecord(value);
  if (!record) return null;
  const label =
    asString(record.label) ?? asString(record.name) ?? asString(record.title);
  if (!label?.trim()) return null;
  const answer =
    asString(record.value) ?? asString(record.answer) ?? asString(record.id) ?? label;
  return { label: label.trim(), value: answer.trim() };
}

/** Recover the safe question/options already carried by the tool summary. */
export function agentQuestion(tool: ToolCallView): AgentQuestion | null {
  if (!isAskTool(tool)) return null;
  let source: Record<string, unknown> | null = null;
  try {
    const parsed: unknown = JSON.parse(tool.input.summary);
    source = asRecord(parsed);
    const questions = source?.questions;
    if (Array.isArray(questions)) source = asRecord(questions[0]) ?? source;
  } catch {
    // A plain-text question is valid and falls back to the title/summary.
  }
  const prompt = (
    asString(source?.question) ??
    asString(source?.prompt) ??
    asString(source?.message) ??
    asString(source?.text) ??
    tool.title.replace(/^ask\s*[:：]?\s*/i, "") ??
    tool.input.summary
  ).trim();
  const rawChoices = source?.choices ?? source?.options;
  const choices = Array.isArray(rawChoices)
    ? rawChoices
        .map(questionChoice)
        .filter((choice): choice is AgentQuestionChoice => choice != null)
        .slice(0, 12)
    : [];
  return { prompt: prompt || "智能体需要你的确认", choices };
}
