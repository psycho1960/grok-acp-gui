import { expect, test } from "./fixtures";

test.describe("GAG-007 first-use closed loop (shell)", () => {
  test("new task skips the setup form and opens an empty conversation", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/#task-center");
    await page.getByTestId("header-create-task").click();
    await expect(page.getByTestId("create-task-form")).toHaveCount(0);
    await expect(page).toHaveURL(/#conversation\//, { timeout: 10_000 });
    await expect(page.getByTestId("conversation-view")).toBeVisible({ timeout: 10_000 });
  });

  test("empty shell offers open project, then create task, then conversation", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    // Shell (not #task-center fixture) uses stateful empty fake bridge
    await page.goto("/#first-use");
    await expect(page.getByTestId("task-center")).toBeVisible({ timeout: 15_000 });

    // No project
    await expect(page.getByTestId("project-empty")).toBeVisible();
    await expect(page.getByTestId("header-create-task")).toBeVisible();
    await page.getByTestId("header-create-task").click();
    await expect(page.getByTestId("open-project-form")).toBeVisible();

    // Cancel selection dialog
    await page.getByTestId("project-open-cancel").click();
    await expect(page.getByTestId("open-project-form")).toHaveCount(0);

    // New task intent carries through project selection directly to conversation.
    await page.getByTestId("header-create-task").click();
    await page.getByTestId("project-path-input").locator("input").fill("D:/work/demo-app");
    await page.getByTestId("project-trust").check();
    await page.getByTestId("project-open-submit").click();

    await expect(page.getByTestId("create-task-form")).toHaveCount(0);
    await expect(page).toHaveURL(/#conversation\//, { timeout: 10_000 });
    await expect(page.getByTestId("conversation-view")).toBeVisible({ timeout: 10_000 });
  });

  test("invalid path shows error and stays without project", async ({ page }) => {
    await page.goto("/#first-use");
    await expect(page.getByTestId("task-center")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("header-open-project").click();
    await page.getByTestId("project-path-input").locator("input").fill("D:/missing/nowhere");
    await page.getByTestId("project-trust").check();
    await page.getByTestId("project-open-submit").click();
    await expect(page.getByTestId("project-open-error")).toBeVisible();
    await expect(page.getByTestId("no-project-label")).toBeVisible();
  });

  test("seeded #task-center fixture still lists seed tasks", async ({ page }) => {
    await page.goto("/#task-center");
    await expect(page.getByTestId("task-center")).toBeVisible();
    await expect(page.locator("[data-task-id]").first()).toBeVisible();
  });
});
