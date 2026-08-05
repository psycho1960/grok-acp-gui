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

/**
 * Convert a restricted Markdown subset to safe HTML.
 * Supports: fenced code, inline code, bold, italic, links, paragraphs, line breaks.
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

  // Restore fenced blocks as <pre><code>
  text = text.replace(/%%FENCE(\d+)%%/g, (_m, n) => {
    const fence = fences[Number(n)];
    if (!fence) return "";
    const langClass = fence.lang
      ? ` class="language-${escapeHtml(fence.lang)}"`
      : "";
    return `<pre class="md-code" data-lang="${escapeHtml(fence.lang)}"><code${langClass}>${escapeHtml(fence.code)}</code></pre>`;
  });

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

  // Paragraphs / line breaks
  const blocks = text.split(/\n{2,}/);
  return blocks
    .map((block) => {
      if (block.startsWith("<pre")) return block;
      const withBreaks = block.replace(/\n/g, "<br />");
      return `<p class="md-p">${withBreaks}</p>`;
    })
    .join("");
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
