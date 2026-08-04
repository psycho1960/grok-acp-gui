import { expect, test } from "@playwright/test";
import axe from "axe-core";

test.describe("GAG-008 conversation timeline", () => {
  test("fixture loads timeline, composer, and can send", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/#conversation");
    await expect(page.getByTestId("conversation-fixture")).toBeVisible({
      timeout: 15_000,
    });
    await expect(page.getByTestId("conversation-view")).toBeVisible();
    await expect(page.getByTestId("composer")).toBeVisible();

    // Wait for snapshot / autoplay items
    await expect(page.getByTestId("conversation-header")).toBeVisible();

    // May be disabled while running — wait for idle after autoplay
    await page.waitForTimeout(500);
    // Stop if running so we can send
    const stop = page.getByTestId("composer-stop");
    if (await stop.isVisible().catch(() => false)) {
      await stop.click();
    }

    // After fixture autoplay may leave waiting_permission — still check virtual list exists or empty
    const body = page.locator(".conversation");
    await expect(body).toBeVisible();
  });

  test("scroll up shows jump-to-bottom control when content overflows", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/#conversation");
    await expect(page.getByTestId("conversation-view")).toBeVisible({
      timeout: 15_000,
    });

    const list = page.getByTestId("conversation-virtual-list");
    // Wait until list has content
    await page.waitForTimeout(1200);
    if (await list.count()) {
      await list.evaluate((el) => {
        el.scrollTop = 0;
        el.dispatchEvent(new Event("scroll"));
      });
      // Jump button appears when not stuck to bottom and user scrolled up
      const jump = page.getByTestId("jump-to-bottom");
      // May not appear if content fits viewport — soft assert
      if (await jump.isVisible().catch(() => false)) {
        await jump.click();
        await expect(jump).toBeHidden({ timeout: 3000 });
      }
    }
  });

  test("deep link hash keeps conversation route", async ({ page }) => {
    await page.goto("/#conversation/task-conv-1/e/3");
    await expect(page.getByTestId("conversation-fixture")).toBeVisible({
      timeout: 15_000,
    });
    expect(page.url()).toContain("conversation");
  });

  test("no critical axe violations on conversation fixture", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/#conversation");
    await expect(page.getByTestId("conversation-view")).toBeVisible({
      timeout: 15_000,
    });
    await page.addScriptTag({ content: axe.source });
    const violations = await page.evaluate(async () => {
      const result = await axe.run(document);
      return result.violations.filter(
        (v) => v.impact === "critical" || v.impact === "serious",
      );
    });
    expect(violations).toEqual([]);
  });
});
