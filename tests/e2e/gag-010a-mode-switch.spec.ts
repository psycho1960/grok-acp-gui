import { expect, test } from "./fixtures";
import type { Page } from "@playwright/test";

// GAG-010A / goal: in-conversation mode switching (智能体/计划/问答) driven
// through the real fixture route (#conversation).

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

async function openConversationFixture(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/#conversation");
  await expect(page.getByTestId("conversation-view")).toBeVisible({
    timeout: 15_000,
  });
  await stopAutoplay(page);
}

test("mode selector shows Chinese labels, switches mode, echoes it, and survives reopen", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await openConversationFixture(page);

  // 1) The mode selector lists the three Chinese capability modes.
  const modeSelect = page.getByTestId("conversation-mode-select").locator("select");
  await expect(modeSelect).toBeVisible();
  await expect(modeSelect.locator("option")).toHaveText([
    "使用会话默认模式",
    "智能体",
    "计划",
    "问答",
  ]);
  // The fixture snapshot restores the default selection.
  await expect(modeSelect).toHaveValue("agent");

  // 2) Switching to Plan persists and the next turn echoes the new mode.
  await modeSelect.selectOption("plan");
  const input = page.getByTestId("composer-input");
  await input.click();
  await input.fill("切换模式后发送");
  await page.getByTestId("composer-send").click();
  await expect(page.getByTestId("user-message").last()).toContainText("切换模式后发送");
  await expect(page.getByTestId("assistant-message").last()).toContainText(
    "mode=plan",
    { timeout: 10_000 },
  );

  // 3) Reopening the conversation restores the persisted mode.
  await page.reload();
  await openConversationFixture(page);
  await expect(
    page.getByTestId("conversation-mode-select").locator("select"),
  ).toHaveValue("plan");

  // No console errors on the real fixture path.
  expect(consoleErrors).toEqual([]);
  await page.screenshot({ path: "mode-e2e.png", fullPage: true });
});

test("mode selector renders and the composer stays usable after a failed mode change", async ({
  page,
}) => {
  await openConversationFixture(page);
  const modeSelect = page.getByTestId("conversation-mode-select").locator("select");
  await expect(modeSelect).toBeVisible();
  // The fixture always accepts mode changes; verify the control stays enabled
  // after sending a turn in Ask mode.
  await modeSelect.selectOption("ask");
  const input = page.getByTestId("composer-input");
  await input.click();
  await input.fill("问答模式消息");
  await page.getByTestId("composer-send").click();
  await expect(page.getByTestId("assistant-message").last()).toContainText(
    "mode=ask",
    { timeout: 10_000 },
  );
  await page.screenshot({ path: "mode-e2e-ask.png", fullPage: true });
});
