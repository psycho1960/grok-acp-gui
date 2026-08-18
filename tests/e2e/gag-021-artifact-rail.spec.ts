import { expect, test } from "./fixtures";

test("timeline artifact opens a populated conversation rail", async ({ page }) => {
  await page.goto("/#conversation");

  const artifact = page.getByRole("button", { name: "screenshot.png", exact: true });
  await expect(artifact).toBeVisible();
  await artifact.click();

  await expect(page.getByText("此任务尚无可用制品。", { exact: true })).toHaveCount(0);
  await expect(page.getByText("screenshot.png", { exact: true })).toHaveCount(2);
  await expect(page.getByText("可用", { exact: true })).toBeVisible();
});
