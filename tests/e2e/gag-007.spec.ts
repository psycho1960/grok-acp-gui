import { expect, test } from "@playwright/test";

test.describe("GAG-007 Task Center fixture", () => {
  test("loads task center via hash with seed tasks", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto("/#task-center");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await expect(page.getByRole("heading", { name: "任务中心" })).toBeVisible();
    await expect(page.locator("[data-task-id]").first()).toBeVisible();
    await expect(page.getByText("等待审批：写入配置")).toBeVisible();
  });

  test("filters tasks by search", async ({ page }) => {
    await page.goto("/#task-center");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await page.getByTestId("task-search").locator("input").fill("中断");
    await expect(page.locator("[data-task-id]")).toHaveCount(1);
    await expect(page.locator('[data-task-id="task-int-1"]')).toBeVisible();
  });

  test("opens detail drawer from deep link and card", async ({ page }) => {
    await page.goto("/#task-center/task-run-1");
    await expect(page.getByTestId("task-detail")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect(page.getByText("实现 Task Center UI").first()).toBeVisible();

    await page.getByLabel("关闭抽屉").click();
    await expect(page.getByTestId("task-detail")).toHaveCount(0);

    await page.locator('[data-task-id="task-wait-1"]').click();
    await expect(page.getByTestId("task-detail")).toBeVisible();
    await expect(page.getByText("等待审批：写入配置").first()).toBeVisible();
  });

  test("cancel confirmation does not optimistically change status", async ({ page }) => {
    await page.goto("/#task-center");
    await expect(page.locator('[data-task-id="task-run-1"]')).toBeVisible();
    const card = page.locator('[data-task-id="task-run-1"]');
    await expect(card).toHaveAttribute("data-status", "running");
    await card.getByTestId("task-cancel").click();
    await expect(page.getByRole("dialog", { name: /确认取消/ })).toBeVisible();
    await expect(card).toHaveAttribute("data-status", "running");
    await page.getByRole("button", { name: "返回" }).click();
    await expect(card).toHaveAttribute("data-status", "running");
  });

  test("supports keyboard open of a task card", async ({ page }) => {
    await page.goto("/#task-center");
    const card = page.locator('[data-task-id="task-run-1"]');
    await card.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("task-detail")).toBeVisible();
  });
});
