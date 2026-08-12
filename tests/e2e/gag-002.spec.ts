import { expect, test } from "./fixtures";
import axe from "axe-core";

const viewports = [
  { width: 1440, height: 900, name: "1440" },
  { width: 1200, height: 900, name: "1200" },
  { width: 1024, height: 680, name: "1024" },
] as const;
const crossPlatformSnapshotTolerance = { maxDiffPixelRatio: 0.005 };
const compactViewportSnapshotTolerance = { maxDiffPixelRatio: 0.01 };

for (const viewport of viewports) {
  test(`captures the shell at ${viewport.name}px`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto("/#shell");
    await expect(page.locator(".app-shell")).toHaveScreenshot(
      `shell-${viewport.name}.png`,
      viewport.name === "1024"
        ? compactViewportSnapshotTolerance
        : crossPlatformSnapshotTolerance,
    );
  });
}

test("keeps navigation reachable at 200% page zoom", async ({ page }) => {
  await page.goto("/#shell");
  const session = await page.context().newCDPSession(page);
  await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 2 });
  await expect(page.getByRole("button", { name: "打开任务导航" })).toBeVisible();
  await expect(page.locator(".shell-left")).toHaveCount(0);
  await page.getByRole("button", { name: "打开任务导航" }).click();
  await expect(page.getByRole("dialog", { name: "任务导航" })).toBeVisible();
  await expect(page.locator(".app-shell")).toHaveScreenshot("shell-200-percent.png");
  await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1 });
});

test("uses a fixed 220px left rail without exposing a false resizer at 1024px", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await page.goto("/#shell");
  await expect(page.getByRole("separator", { name: "调整左侧栏宽度" })).toHaveCount(0);
  await expect(page.locator(".shell-left")).toHaveCSS("width", "220px");
  expect(await page.locator(".shell-main").evaluate((element) => element.clientWidth)).toBeGreaterThan(500);
  await expect(page.getByRole("button", { name: "新建任务" })).toBeVisible();
});

test("keeps every IconButton state in the UI Kit", async ({ page }) => {
  await page.goto("/#ui-kit");
  await expect(page.locator('[aria-label^="图标按钮"]')).toHaveCount(7);
});

test("reports no critical or serious axe violations", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/#shell");
  await page.addScriptTag({ content: axe.source });
  const violations = await page.evaluate(async () => {
    const result = await axe.run(document);
    return result.violations.filter((violation) => violation.impact === "critical" || violation.impact === "serious");
  });
  expect(violations).toEqual([]);
});
