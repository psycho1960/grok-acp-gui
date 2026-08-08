// GAG-010: Task title derivation — pure logic.
// The title is optional; when omitted it is distilled from the first sentence
// of the user's first message (never a fixed placeholder like "未命名任务").

export const DERIVED_TITLE_MAX_CHARS = 30;

const SENTENCE_BOUNDARIES = /[。！？.!?;；]/;

/**
 * Derive a task title from the user's first message:
 * 1. take the first non-empty line,
 * 2. cut it at the first sentence boundary,
 * 3. truncate to `DERIVED_TITLE_MAX_CHARS` with an ellipsis when needed.
 * Mirrors `derive_task_title` in src-tauri/src/bridge/dispatch.rs.
 */
export function deriveTaskTitle(prompt: string): string {
  const firstLine =
    prompt
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find((line) => line.length > 0) ?? "";

  const firstSentence = firstLine
    .split(SENTENCE_BOUNDARIES)
    .map((sentence) => sentence.trim())
    .find((sentence) => sentence.length > 0);

  const candidate = (firstSentence && firstSentence.length > 0
    ? firstSentence
    : firstLine.trim()) || prompt.trim();

  if (candidate.length === 0) return "新任务";
  if (candidate.length <= DERIVED_TITLE_MAX_CHARS) return candidate;
  return `${candidate.slice(0, DERIVED_TITLE_MAX_CHARS).trimEnd()}…`;
}
