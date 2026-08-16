// GAG-008: Safe Markdown → HTML. Disables dangerous HTML and strips bad protocols.

const ALLOWED_PROTOCOLS = new Set(["http:", "https:", "mailto:"]);

/** Defense-in-depth redaction for content rendered or copied in Renderer. */
export function redactVisibleText(source: string): string {
  const sensitive =
    /(xai_api_key|api[_-]?key|apikey|authorization|password|passwd|credential|private[_-]?key|secret|token|cookie)\s*([=:])\s*(?:"[^"]*"|'[^']*'|[^\s]+)/gi;
  return source
    .replace(/\bbearer\s+[^\s]+/gi, "Bearer [redacted]")
    .replace(sensitive, (_match, key: string, separator: string) =>
      `${key}${separator} [redacted]`,
    );
}

/** Escape HTML special characters. */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** Return href only if protocol is allowlisted; otherwise null. */
export function sanitizeHref(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  // Block javascript:, data:, vbscript:, etc. (strip leading whitespace / BOM)
  const normalized = trimmed.replace(/^[\s\uFEFF]+/, "");
  if (/^javascript\s*:/i.test(normalized)) return null;
  if (/^data\s*:/i.test(normalized)) return null;
  if (/^vbscript\s*:/i.test(normalized)) return null;
  try {
    // Relative paths and fragments
    if (trimmed.startsWith("#") || trimmed.startsWith("/")) {
      return escapeHtml(trimmed);
    }
    const url = new URL(trimmed, "https://example.invalid");
    if (!ALLOWED_PROTOCOLS.has(url.protocol)) return null;
    // If original was absolute-looking, return escaped original path form
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed)) {
      const abs = new URL(trimmed);
      if (!ALLOWED_PROTOCOLS.has(abs.protocol)) return null;
      return escapeHtml(abs.href);
    }
    return escapeHtml(trimmed);
  } catch {
    return null;
  }
}

interface CodeFence {
  lang: string;
  code: string;
}

function renderFence(fence: CodeFence | undefined): string {
  if (!fence) return "";
  const langClass = fence.lang
    ? ` class="language-${escapeHtml(fence.lang)}"`
    : "";
  return `<pre class="md-code" data-lang="${escapeHtml(fence.lang)}"><code${langClass}>${escapeHtml(fence.code)}</code></pre>`;
}

function tableCells(line: string): string[] {
  let content = line.trim();
  if (content.startsWith("|")) content = content.slice(1);
  if (content.endsWith("|")) content = content.slice(0, -1);
  return content.split(/(?<!\\)\|/).map((cell) => cell.trim().replace(/\\\|/g, "|"));
}

function tableAlignments(line: string): Array<"left" | "center" | "right"> | null {
  const cells = tableCells(line);
  if (!cells.length || !cells.every((cell) => /^:?-{3,}:?$/.test(cell.replace(/\s/g, "")))) {
    return null;
  }
  return cells.map((cell) => {
    const marker = cell.replace(/\s/g, "");
    if (marker.startsWith(":") && marker.endsWith(":")) return "center";
    if (marker.endsWith(":")) return "right";
    return "left";
  });
}

function isBlockStart(lines: string[], index: number): boolean {
  const line = lines[index] ?? "";
  if (/^%%FENCE\d+%%$/.test(line.trim())) return true;
  if (/^#{1,6}\s+\S/.test(line)) return true;
  if (/^\s*(?:[-+*]\s+|\d+[.)]\s+)\S/.test(line)) return true;
  if (/^\s*>\s?/.test(line)) return true;
  if (/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) return true;
  return index + 1 < lines.length && line.includes("|") && tableAlignments(lines[index + 1] ?? "") !== null;
}

function renderBlocks(text: string, fences: CodeFence[]): string {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const output: string[] = [];

  for (let index = 0; index < lines.length;) {
    const line = lines[index] ?? "";
    if (!line.trim()) {
      index += 1;
      continue;
    }

    const fenceMatch = line.trim().match(/^%%FENCE(\d+)%%$/);
    if (fenceMatch) {
      output.push(renderFence(fences[Number(fenceMatch[1])]));
      index += 1;
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.+?)\s*#*$/);
    if (heading) {
      const level = heading[1]?.length ?? 1;
      output.push(`<h${level} class="md-heading md-h${level}">${heading[2]}</h${level}>`);
      index += 1;
      continue;
    }

    if (/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      output.push('<hr class="md-rule" />');
      index += 1;
      continue;
    }

    const alignments = tableAlignments(lines[index + 1] ?? "");
    if (line.includes("|") && alignments) {
      const headers = tableCells(line);
      const rows: string[][] = [];
      index += 2;
      while (index < lines.length && (lines[index] ?? "").trim() && (lines[index] ?? "").includes("|")) {
        rows.push(tableCells(lines[index] ?? ""));
        index += 1;
      }
      const headerHtml = headers
        .map((cell, cellIndex) => `<th class="md-align-${alignments[cellIndex] ?? "left"}">${cell}</th>`)
        .join("");
      const bodyHtml = rows
        .map((row) => `<tr>${headers.map((_header, cellIndex) => `<td class="md-align-${alignments[cellIndex] ?? "left"}">${row[cellIndex] ?? ""}</td>`).join("")}</tr>`)
        .join("");
      output.push(`<div class="md-table-wrap"><table class="md-table"><thead><tr>${headerHtml}</tr></thead><tbody>${bodyHtml}</tbody></table></div>`);
      continue;
    }

    const unordered = line.match(/^\s*[-+*]\s+(.+)$/);
    const ordered = line.match(/^\s*\d+[.)]\s+(.+)$/);
    if (unordered || ordered) {
      const orderedList = Boolean(ordered);
      const pattern = orderedList ? /^\s*\d+[.)]\s+(.+)$/ : /^\s*[-+*]\s+(.+)$/;
      const items: string[] = [];
      while (index < lines.length) {
        const item = (lines[index] ?? "").match(pattern);
        if (!item) break;
        items.push(`<li>${item[1]}</li>`);
        index += 1;
      }
      const tag = orderedList ? "ol" : "ul";
      output.push(`<${tag} class="md-list">${items.join("")}</${tag}>`);
      continue;
    }

    if (/^\s*>\s?/.test(line)) {
      const quoted: string[] = [];
      while (index < lines.length) {
        const quote = (lines[index] ?? "").match(/^\s*>\s?(.*)$/);
        if (!quote) break;
        quoted.push(quote[1] ?? "");
        index += 1;
      }
      output.push(`<blockquote class="md-quote">${quoted.join("<br />")}</blockquote>`);
      continue;
    }

    const paragraph: string[] = [];
    while (index < lines.length && (lines[index] ?? "").trim() && !isBlockStart(lines, index)) {
      paragraph.push(lines[index] ?? "");
      index += 1;
    }
    if (paragraph.length) {
      output.push(`<p class="md-p">${paragraph.join("<br />")}</p>`);
      continue;
    }

    index += 1;
  }

  return output.join("");
}

/**
 * Convert a restricted Markdown subset to safe HTML.
 * Supports: headings, tables, lists, blockquotes, rules, fenced code, inline code,
 * bold, italic, links, paragraphs, and line breaks.
 * Raw HTML in source is escaped, never executed.
 */
export function renderSafeMarkdown(source: string): string {
  if (!source) return "";

  // Extract fenced code blocks first so inner markdown is not processed.
  const fences: CodeFence[] = [];
  const FENCE_OPEN = "%%FENCE";
  const FENCE_CLOSE = "%%";
  let text = redactVisibleText(source).replace(/```([^\n`]*)\n?([\s\S]*?)```/g, (_m, lang, code) => {
    const idx = fences.length;
    fences.push({
      lang: String(lang || "").trim().slice(0, 32),
      code: String(code ?? "").replace(/\n$/, ""),
    });
    return `${FENCE_OPEN}${idx}${FENCE_CLOSE}`;
  });

  // Escape everything remaining (kills injected HTML / scripts).
  text = escapeHtml(text);

  // Inline code
  text = text.replace(/`([^`\n]+)`/g, (_m, code) => {
    return `<code class="md-inline">${code}</code>`;
  });

  // Bold then italic (order matters)
  text = text.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  text = text.replace(/(?<!\*)\*([^*\n]+)\*(?!\*)/g, "<em>$1</em>");

  // Links [label](url)
  text = text.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_m, label, href) => {
    const safe = sanitizeHref(String(href));
    if (!safe) {
      return escapeHtml(String(label));
    }
    return `<a href="${safe}" rel="noopener noreferrer" target="_blank">${label}</a>`;
  });

  // Bare autolink-ish http(s) URLs (after escape, still plain text)
  text = text.replace(
    /(^|[\s>(])(https?:\/\/[^\s<]+)/g,
    (_m, pre, url) => {
      const safe = sanitizeHref(url);
      if (!safe) return `${pre}${url}`;
      return `${pre}<a href="${safe}" rel="noopener noreferrer" target="_blank">${url}</a>`;
    },
  );

  return renderBlocks(text, fences);
}

/** Complete visible plain text for the default copy action. */
export function visiblePlainText(source: string): string {
  return redactVisibleText(source)
    .replace(/```[^\n`]*\n?([\s\S]*?)```/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[*_>#]/g, "");
}

/** Visible plain-text summary for compact diagnostic surfaces. */
export function plainTextSummary(source: string, maxLen = 4000): string {
  const plain = visiblePlainText(source);
  if (plain.length <= maxLen) return plain;
  return `${plain.slice(0, maxLen)}…`;
}
