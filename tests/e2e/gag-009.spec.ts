import { expect, test } from "@playwright/test";

test.describe("GAG-009 permission and Plan approval", () => {
  test("shows impact, defaults keyboard focus to a safe option, and blocks double submit", async ({ page }) => {
    await page.goto("/#conversation");
    const permission = page.getByTestId("permission-slot");
    await expect(permission).toBeVisible({ timeout: 15_000 });
    await expect(permission).toContainText("将修改工作区");
    await expect(permission).toContainText("src/app.ts");
    await expect(page.getByTestId("plan-slot")).toBeVisible();

    await page.keyboard.press("Control+.");
    await expect(page.getByRole("button", { name: "继续规划" })).toBeFocused();

    const reject = permission.getByRole("button", { name: "Reject" });
    await reject.dblclick();
    await expect(permission.getByText("决定已提交")).toBeVisible();
    await expect(permission.getByRole("button", { name: "Allow once" })).toBeDisabled();
  });

  test("renders versioned Plan steps and resolves the exact capability option", async ({ page }) => {
    await page.goto("/#conversation");
    const plan = page.getByTestId("plan-slot");
    await expect(plan).toBeVisible({ timeout: 15_000 });
    await expect(plan).toContainText("规划阶段：写入与非只读命令已阻止");
    await expect(plan.locator("ol li")).toHaveCount(3);
    await plan.getByRole("button", { name: "继续规划" }).click();
    await expect(plan.getByRole("button", { name: "批准" })).toBeDisabled();
  });
});
