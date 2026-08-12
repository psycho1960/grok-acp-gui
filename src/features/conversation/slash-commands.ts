// GAG-010: "/" quick-command menu — pure logic for matching, filtering, and
// inserting grok build slash commands (discovered from ACP available_commands).

import type { SlashCommandInfo } from "../../bridge/types";

export interface SlashMenuState {
  /** Whether the caret is on a line that starts with "/". */
  open: boolean;
  /** Text after "/" used to filter commands. */
  query: string;
  /** Character offset where the current line starts. */
  lineStart: number;
  /** Character offset just past the "/". */
  slashOffset: number;
}

/**
 * Compute the slash-menu state for `text` with the caret at `cursor`.
 * The menu opens when the current line (from the last newline to the caret)
 * starts with "/". Filtering is case-insensitive on the text after "/";
 * typing whitespace after the command name closes the menu (command names
 * never contain spaces).
 */
export function slashMenuState(
  text: string,
  cursor: number,
): SlashMenuState {
  const lineStart = text.lastIndexOf("\n", cursor - 1) + 1;
  const linePrefix = text.slice(lineStart, cursor);
  const trimmed = linePrefix.trimStart();
  const leadingWhitespace = linePrefix.length - trimmed.length;
  const slashOffset = lineStart + leadingWhitespace;
  if (!trimmed.startsWith("/")) {
    return { open: false, query: "", lineStart, slashOffset };
  }
  const query = trimmed.slice(1);
  if (/\s/.test(query)) {
    return { open: false, query: "", lineStart, slashOffset };
  }
  return {
    open: true,
    query,
    lineStart,
    slashOffset,
  };
}

/** Filter commands by prefix (case-insensitive, name first). */
export function filterSlashCommands(
  commands: readonly SlashCommandInfo[],
  query: string,
): SlashCommandInfo[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...commands];
  return commands.filter((command) =>
    command.name.toLowerCase().includes(q),
  );
}

export type InsertSlashMode = "replace-slash-line" | "insert-at-cursor";

/**
 * Insert a slash command into `text`.
 * - `replace-slash-line` (default): replace the current "/..." token (typing flow).
 * - `insert-at-cursor`: insert at the caret without wiping the rest of the line
 *   (command palette / help button when the line is not a slash draft).
 */
export function insertSlashCommand(
  text: string,
  cursor: number,
  commandName: string,
  mode: InsertSlashMode = "replace-slash-line",
): { text: string; cursor: number } {
  const commandText = `/${commandName}`;
  const state = slashMenuState(text, cursor);

  if (mode === "insert-at-cursor" || !state.open) {
    const before = text.slice(0, cursor);
    const after = text.slice(cursor);
    const needsSpace =
      before.length > 0 && !/\s$/.test(before) && !before.endsWith("\n");
    const insert = `${needsSpace ? " " : ""}${commandText}`;
    return { text: before + insert + after, cursor: before.length + insert.length };
  }

  const lineEnd = text.indexOf("\n", cursor);
  const end = lineEnd < 0 ? text.length : lineEnd;
  const next = text.slice(0, state.lineStart) + commandText + text.slice(end);
  return { text: next, cursor: state.lineStart + commandText.length };
}
