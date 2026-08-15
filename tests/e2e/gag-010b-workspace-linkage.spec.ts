import { expect, test, waitForConversationIdle } from "./fixtures";
import type { Page } from "@playwright/test";

// GAG-010B / goal: mode ↔ workspace strategy linkage driven through the real
// fixture route (#conversation).

async function openConversationFixture(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/#conversation");
  await expect(page.getByTestId("conversation-view")).toBeVisible({
    timeout: 15_000,
  });
  await waitForConversationIdle(page);
}

test("mode switch links the workspace strategy, echoes it, and survives reopen", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await openConversationFixture(page);

  await expect(page.getByTestId("conversation-workspace-notice")).toHaveText(
    "隔离 Worktree 尚未创建，本任务不会回落到原工作区。",
  );

  // 1) The workspace strategy selector lists the three Chinese labels.
  const workspaceSelect = page
    .getByTestId("conversation-workspace-select")
    .locator("select");
  await expect(workspaceSelect).toBeVisible();
  await expect(workspaceSelect.locator("option")).toHaveText([
    "使用创建时的策略",
    "隔离 Worktree",
    "只读当前目录",
    "当前目录可写",
  ]);

  // 2) Switching mode ask links the strategy to direct (当前目录可写).
  const modeSelect = page.getByTestId("conversation-mode-select").locator("select");
  await modeSelect.selectOption("ask");
  await expect(workspaceSelect).toHaveValue("direct");

  // 3) The next turn echoes both the mode and the linked strategy.
  const input = page.getByTestId("composer-input");
  await input.click();
  await input.fill("联动策略消息");
  await page.getByTestId("composer-send").click();
  await expect(page.getByTestId("user-message").last()).toContainText("联动策略消息");
  await expect(page.getByTestId("assistant-message").last()).toContainText(
    "mode=ask workspace=direct",
    { timeout: 10_000 },
  );

  // 4) Manual strategy change persists too.
  await workspaceSelect.selectOption("readonly");
  await input.click();
  await input.fill("手动策略消息");
  await page.getByTestId("composer-send").click();
  await expect(page.getByTestId("assistant-message").last()).toContainText(
    "workspace=readonly",
    { timeout: 10_000 },
  );

  // 5) Reopening restores mode + strategy.
  await page.reload();
  await openConversationFixture(page);
  await expect(modeSelect).toHaveValue("ask");
  await expect(workspaceSelect).toHaveValue("readonly");

  expect(consoleErrors).toEqual([]);
  await page.screenshot({ path: "workspace-e2e.png", fullPage: true });
});

test("switching to plan links the strategy to worktree", async ({ page }) => {
  await openConversationFixture(page);
  const workspaceSelect = page
    .getByTestId("conversation-workspace-select")
    .locator("select");
  const modeSelect = page.getByTestId("conversation-mode-select").locator("select");

  await modeSelect.selectOption("plan");
  await expect(workspaceSelect).toHaveValue("worktree");
  await page.screenshot({ path: "workspace-e2e-plan.png", fullPage: true });
});
