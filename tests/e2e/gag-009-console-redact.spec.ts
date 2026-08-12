import { expect, test } from "./fixtures";

// B-2 (RG-009-X-05): the browser console must never expose tokens,
// API keys, environment values, or test secrets. The backend redacts
// server-side, but client-side code may still log debug strings;
// assert nothing secret-like appears on `page.on('console', ...)`.

test.describe("GAG-009 browser console secret redaction", () => {
  test("console emits no tokens, Bearer headers, or test secrets", async ({ page }) => {
    const consoleLines: string[] = [];
    const pageErrors: string[] = [];
    page.on("console", (msg) => {
      consoleLines.push(msg.text());
    });
    page.on("pageerror", (err) => {
      pageErrors.push(err.message);
    });

    await page.goto("/#conversation");
    const permission = page.getByTestId("permission-slot");
    await expect(permission).toBeVisible({ timeout: 15_000 });

    // Exercise user-style interactions that previously risked leaking raw
    // values into the DOM / DevTools.
    await expect(permission).toContainText("[redacted]");
    await page.keyboard.press("Control+.");

    // Trigger one more navigation that re-renders the timeline.
    await page.goto("/#conversation");
    await expect(page.getByTestId("permission-slot")).toBeVisible({ timeout: 15_000 });

    // Any uncaught page error is itself a console leak — fail loudly.
    expect(pageErrors, "page errors should not include secrets").toEqual([]);

    const secretPatterns: RegExp[] = [
      /GAG009_TEST_SECRET_NEVER_LOG/,
      /sk-[a-zA-Z0-9]{16,}/,
      /Bearer\s+[a-zA-Z0-9._-]+/i,
      /XAI_API_KEY\s*=\s*[A-Za-z0-9._-]+/,
    ];

    for (const line of consoleLines) {
      for (const pattern of secretPatterns) {
        expect(
          line,
          `console line matched secret pattern ${pattern}: ${line}`,
        ).not.toMatch(pattern);
      }
    }
  });
});
