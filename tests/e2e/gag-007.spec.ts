import { expect, test } from "./fixtures";

type FixtureWindow = Window & {
  __taskCenterPushState?: (taskId: string, status: string, seq?: number) => void;
  __taskCenterFailCancel?: boolean;
};

test.describe("GAG-007 Task Center fixture", () => {
  test("group headers use compact rows instead of task-card height", async ({ page }) => {
    await page.goto("/#task-center");

    const waitingHeader = page.locator('[data-group-header="needs_attention"]');
    const runningHeader = page.locator('[data-group-header="running"]');
    await expect(waitingHeader).toBeVisible();
    await expect(runningHeader).toBeVisible();

    const waitingBox = await waitingHeader.boundingBox();
    const runningBox = await runningHeader.boundingBox();
    expect(waitingBox).not.toBeNull();
    expect(runningBox).not.toBeNull();
    expect(waitingBox!.height).toBeLessThanOrEqual(48);
    expect(runningBox!.y - waitingBox!.y).toBeLessThan(190);
  });

  test("loads task center via hash with seed tasks", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto("/#task-center");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await expect(page.getByRole("heading", { name: "任务中心" })).toBeVisible();
    await expect(page.locator("[data-task-id]").first()).toBeVisible();
    await expect(page.getByText("等待审批：写入配置")).toBeVisible();
    await expect(page.locator("[data-group-header]").first()).toBeVisible();
  });

  test("filters tasks by search", async ({ page }) => {
    await page.goto("/#task-center");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await page.getByTestId("task-search").locator("input").fill("中断");
    await expect(page.locator("[data-task-id]")).toHaveCount(1);
    await expect(page.locator('[data-task-id="task-int-1"]')).toBeVisible();
  });

  test("opens detail drawer from deep link and card open control", async ({ page }) => {
    await page.goto("/#task-center/task-run-1");
    await expect(page.getByTestId("task-detail")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect(page.getByText("实现 Task Center UI").first()).toBeVisible();

    await page.getByLabel("关闭抽屉").click();
    await expect(page.getByTestId("task-detail")).toHaveCount(0);

    await page.locator('[data-task-id="task-wait-1"] [data-testid="task-open"]').click();
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

  test("cancel failure shows alert and keeps running status", async ({ page }) => {
    await page.goto("/#task-center");
    await page.evaluate(() => {
      (window as FixtureWindow).__taskCenterFailCancel = true;
    });
    const card = page.locator('[data-task-id="task-run-1"]');
    await card.getByTestId("task-cancel").click();
    await page.getByTestId("confirm-cancel").click();
    await expect(page.getByTestId("cancel-feedback")).toContainText("取消失败");
    await expect(card).toHaveAttribute("data-status", "running");
  });

  test("live status change updates card status via fixture push", async ({ page }) => {
    await page.goto("/#task-center");
    const card = page.locator('[data-task-id="task-run-1"]');
    await expect(card).toHaveAttribute("data-status", "running");
    await page.evaluate(() => {
      (window as FixtureWindow).__taskCenterPushState?.("task-run-1", "waiting_permission", 50);
    });
    await expect(card).toHaveAttribute("data-status", "waiting_permission");
    await expect(page.getByTestId("task-live-region")).toContainText("等待审批");
  });

  test("supports keyboard open of a task card", async ({ page }) => {
    await page.goto("/#task-center");
    const open = page.locator('[data-task-id="task-run-1"] [data-testid="task-open"]');
    await open.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("task-detail")).toBeVisible();
  });

  test("group query deep link focuses group filter", async ({ page }) => {
    await page.goto("/#task-center?group=failed_interrupted");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await expect(page.locator("[data-task-id]")).toHaveCount(1);
    await expect(page.locator('[data-task-id="task-int-1"]')).toBeVisible();
  });

  test("in-progress and completed navigation filters show distinct task sets", async ({ page }) => {
    await page.goto("/#task-center?group=running");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await expect(page.locator("[data-task-id]")).toHaveCount(2);
    await expect(page.locator('[data-task-id="task-run-1"]')).toBeVisible();
    await expect(page.locator('[data-task-id="task-merged-1"]')).toHaveCount(0);

    await page.evaluate(() => {
      (window as FixtureWindow).__taskCenterPushState?.("task-run-1", "idle", 90);
    });
    await expect(page.locator("[data-task-id]")).toHaveCount(1);
    await expect(page.locator('[data-task-id="task-run-1"]')).toHaveCount(0);

    await page.evaluate(() => {
      window.location.hash = "#task-center?group=completed";
    });
    await expect(page.locator("[data-task-id]")).toHaveCount(2);
    await expect(page.locator('[data-task-id="task-run-1"]')).toBeVisible();
    await expect(page.locator('[data-task-id="task-merged-1"]')).toBeVisible();
  });

  test("navigating to bare #task-center clears group filter", async ({ page }) => {
    await page.goto("/#task-center?group=failed_interrupted");
    await expect(page.locator("[data-task-id]")).toHaveCount(1);
    await page.evaluate(() => {
      window.location.hash = "#task-center";
    });
    await expect(page.locator("[data-task-id]").first()).toBeVisible();
    await expect.poll(async () => page.locator("[data-task-id]").count()).toBeGreaterThan(1);
  });

  test("open detail reflects live task.state without re-open", async ({ page }) => {
    await page.goto("/#task-center/task-run-1");
    await expect(page.getByTestId("task-detail")).toBeVisible();
    await expect(page.locator('[data-task-id="task-run-1"]')).toHaveAttribute(
      "data-status",
      "running",
    );
    await page.evaluate(() => {
      (window as FixtureWindow).__taskCenterPushState?.("task-run-1", "interrupted", 80);
    });
    await expect(page.locator('[data-task-id="task-run-1"]')).toHaveAttribute(
      "data-status",
      "interrupted",
    );
    // Drawer should show interrupted affordances (recover) after live update.
    await expect(page.getByTestId("detail-recover")).toBeVisible();
  });
});
