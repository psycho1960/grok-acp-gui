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
      await expect(
        page
          .getByTestId("conversation-header")
          .locator(".badge")
          .filter({ hasText: /^空闲$/ }),
      ).toBeVisible();
    }

    const input = page.getByTestId("composer-input");
    await input.fill("one visible user turn");
    await page.getByTestId("composer-send").click();
    await expect(page.getByTestId("user-message").filter({ hasText: "one visible user turn" })).toHaveCount(1);
    await expect(page.getByTestId("assistant-message").filter({ hasText: "回复：one visible user turn" })).toBeVisible();
    await expect(page.getByText("空闲", { exact: true }).first()).toBeVisible();
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
    await page.waitForTimeout(1200);
    await list.evaluate((el) => {
      Object.defineProperty(el, "clientHeight", { configurable: true, value: 200 });
      Object.defineProperty(el, "scrollHeight", { configurable: true, value: 1000 });
      el.scrollTop = 0;
      el.dispatchEvent(new Event("scroll"));
    });

    const jump = page.getByTestId("jump-to-bottom");
    await expect(jump).toBeVisible();

    const stop = page.getByTestId("composer-stop");
    if (await stop.isVisible()) {
      await stop.click();
      await expect(
        page
          .getByTestId("conversation-header")
          .locator(".badge")
          .filter({ hasText: /^空闲$/ }),
      ).toBeVisible();
    }

    const input = page.getByTestId("composer-input");
    await input.fill("new content while reading history");
    await page.getByTestId("composer-send").click();
    await expect(page.getByTestId("unread-count")).toHaveText(/^[1-9]\d*$/);
    expect(await list.evaluate((el) => el.scrollTop)).toBe(0);

    await jump.click();
    await expect(jump).toBeHidden({ timeout: 3000 });
  });

  test("refresh restores the nearby reading position", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/#conversation");
    const list = page.getByTestId("conversation-virtual-list");
    await expect(list).toBeVisible({ timeout: 15_000 });
    await page.waitForTimeout(1200);

    await list.evaluate((el) => {
      Object.defineProperty(el, "clientHeight", { configurable: true, value: 200 });
      Object.defineProperty(el, "scrollHeight", { configurable: true, value: 1000 });
      el.scrollTop = 100;
      el.dispatchEvent(new Event("scroll"));
    });
    await expect(page.getByTestId("jump-to-bottom")).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(() => {
          const key = Object.keys(window.localStorage).find((candidate) =>
            candidate.startsWith("gag008:scroll:"),
          );
          if (!key) return null;
          return JSON.parse(window.localStorage.getItem(key) ?? "null") as {
            scrollTop?: number;
            stickToBottom?: boolean;
          };
        }),
      )
      .toMatchObject({ scrollTop: 100, stickToBottom: false });

    await page.reload();
    const restored = page.getByTestId("conversation-virtual-list");
    await expect(restored).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("jump-to-bottom")).toBeVisible();
    await expect
      .poll(() => restored.evaluate((el) => el.scrollTop), { timeout: 5_000 })
      .toBeGreaterThanOrEqual(90);
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
