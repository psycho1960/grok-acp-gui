import { describe, expect, it } from "vitest";
import {
  escapeHtml,
  plainTextSummary,
  renderSafeMarkdown,
  sanitizeHref,
  redactVisibleText,
} from "../../src/features/conversation/markdown";

describe("GAG-008 safe markdown", () => {
  it("escapes raw HTML and scripts", () => {
    const html = renderSafeMarkdown(`Hello <script>alert(1)</script> **bold**`);
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("<strong>bold</strong>");
  });

  it("blocks javascript and data URLs in links", () => {
    expect(sanitizeHref("javascript:alert(1)")).toBeNull();
    expect(sanitizeHref("data:text/html,hi")).toBeNull();
    expect(sanitizeHref("https://example.com/a")).toContain("https://example.com/a");
    const html = renderSafeMarkdown(`[x](javascript:alert(1)) [ok](https://x.test)`);
    expect(html).not.toContain("javascript:");
    expect(html).toContain('href="https://x.test/"');
  });

  it("renders fenced code without interpreting inner markdown/html", () => {
    const src = "```js\nconst x = `<img onerror=alert(1)>`;\n```";
    const html = renderSafeMarkdown(src);
    expect(html).toContain("<pre");
    expect(html).toContain("&lt;img");
    expect(html).not.toContain("<img");
  });

  it("handles long unbroken text without throwing", () => {
    const long = "a".repeat(50_000);
    const html = renderSafeMarkdown(long);
    expect(html.length).toBeGreaterThan(1000);
    expect(plainTextSummary(long, 100).length).toBeLessThanOrEqual(101);
  });

  it("escapeHtml is total for control chars used in XSS", () => {
    expect(escapeHtml(`<"&'>`)).toBe("&lt;&quot;&amp;&#39;&gt;");
  });

  it("redacts common credentials from visible and copied text", () => {
    const source = "XAI_API_KEY=super-secret-value Authorization: Bearer abc.def.ghi";
    const redacted = redactVisibleText(source);
    expect(redacted).toContain("[redacted]");
    expect(redacted).not.toContain("super-secret-value");
    expect(redacted).not.toContain("abc.def.ghi");
    expect(plainTextSummary(source)).not.toContain("super-secret-value");
  });
});
