import { expect, test, type Page } from "@playwright/test";

// GAG-010A / goal: clipboard screenshot paste, in-conversation model &
// reasoning switching, and the "/" quick-command menu — driven through the
// real fixture route (#conversation) with the stateful fake bridge.

/** Stop the autoplay fixture turn so the composer becomes usable. */
async function stopAutoplay(page: Page): Promise<void> {
  const stop = page.getByTestId("composer-stop");
  if (await stop.isVisible().catch(() => false)) {
    await stop.click();
    await expect(
      page
        .getByTestId("conversation-header")
        .locator(".badge")
        .filter({ hasText: /^空闲$/ }),
    ).toBeVisible({ timeout: 10_000 });
  }
}

/** Paste a tiny PNG through a real ClipboardEvent with a DataTransfer. */
async function pasteClipboardImage(page: Page, fileName = "image.png"): Promise<void> {
  await page.evaluate((name) => {
    const pngBytes = new Uint8Array([
      137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1,
      0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84,
      8, 215, 99, 100, 96, 248, 95, 15, 0, 3, 5, 129, 128, 112, 174, 48, 138, 0,
      0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ]);
    const dt = new DataTransfer();
    dt.items.add(new File([pngBytes], name, { type: "image/png" }));
    const input = document.querySelector(
      '[data-testid="composer-input"]',
    ) as HTMLTextAreaElement | null;
    if (!input) throw new Error("composer input not found");
    input.dispatchEvent(
      new ClipboardEvent("paste", {
        clipboardData: dt,
        bubbles: true,
        cancelable: true,
      }),
    );
  }, fileName);
}

async function openConversationFixture(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/#conversation");
  await expect(page.getByTestId("conversation-view")).toBeVisible({
    timeout: 15_000,
  });
  await stopAutoplay(page);
}

test("clipboard paste, slash commands, and model switching work together", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await openConversationFixture(page);
  await expect(page.getByTestId("conversation-header")).toBeVisible();

  // 1) Paste a clipboard screenshot → attachment chip appears.
  await pasteClipboardImage(page, "截图.png");
  await expect(page.getByTestId("composer").getByText("截图.png")).toBeVisible();
  await expect(page.getByTestId("composer")).toContainText("KiB");

  // 2) "/" opens the quick-command menu; prefix filters; Enter inserts.
  const input = page.getByTestId("composer-input");
  await input.click();
  await input.pressSequentially("/pl", { delay: 20 });
  const menu = page.getByTestId("slash-menu");
  await expect(menu).toBeVisible();
  await expect(menu.getByTestId("slash-menu-item")).toHaveCount(1);
  await expect(menu.getByText("/plan")).toBeVisible();
  await input.press("Enter");
  await expect(input).toHaveValue("/plan");

  // Esc closes the menu without cancelling; a space after the command
  // keeps the menu closed.
  await input.press("Escape");
  await input.pressSequentially(" 再来", { delay: 10 });

  // 3) Switch model + reasoning; the next turn echoes the new selection.
  await page
    .getByTestId("conversation-model-select")
    .locator("select")
    .selectOption("deepseek");
  await page
    .getByTestId("conversation-reasoning-select")
    .locator("select")
    .selectOption("max");

  await page.getByTestId("composer-send").click();
  await expect(page.getByTestId("user-message").last()).toContainText("/plan 再来");  // The fixture echoes what the turn carried: [model=deepseek reasoning=max].
  await expect(
    page.getByTestId("assistant-message").last(),
  ).toContainText("model=deepseek reasoning=max", { timeout: 10_000 });

  // Reopening the conversation restores the selection.
  await page.reload();
  await openConversationFixture(page);
  await expect(
    page.getByTestId("conversation-model-select").locator("select"),
  ).toHaveValue("deepseek");
  await expect(
    page.getByTestId("conversation-reasoning-select").locator("select"),
  ).toHaveValue("max");

  await expect(page.getByTestId("conversation-view")).toBeVisible();
  await page.screenshot({ path: "composer-e2e.png", fullPage: true });

  expect(consoleErrors).toEqual([]);
});

test("slash menu filters, navigates with arrows, and closes on Esc", async ({
  page,
}) => {
  await openConversationFixture(page);
  const input = page.getByTestId("composer-input");
  await input.click();

  // Bare "/" lists all seeded commands.
  await input.pressSequentially("/", { delay: 20 });
  const menu = page.getByTestId("slash-menu");
  await expect(menu).toBeVisible();
  await expect(menu.getByTestId("slash-menu-item")).toHaveCount(3);

  // ArrowDown moves the highlight; Enter picks the second command.
  await input.press("ArrowDown");
  await input.press("Enter");
  const value = await input.inputValue();
  expect(value.startsWith("/")).toBe(true);
  expect(value.length).toBeGreaterThan(1);

  // Esc closes the menu without sending; the draft stays untouched.
  await input.press("Escape");
  await expect(menu).toBeHidden();
  expect(await input.inputValue()).toBe("/plan");
  await input.fill("");
  await input.press("Enter");
  await expect(page.getByTestId("user-message")).toHaveCount(0);
  await page.screenshot({ path: "slash-menu-e2e.png", fullPage: true });
});
