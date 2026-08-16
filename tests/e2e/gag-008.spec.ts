import { expect, test, waitForConversationIdle } from "./fixtures";
import axe from "axe-core";

test.describe("GAG-008 conversation timeline", () => {
  test("conversation scrollbars use the dark theme instead of Windows native chrome", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/#conversation");
    const timeline = page.getByTestId("conversation-virtual-list");
    await expect(timeline).toBeVisible({ timeout: 15_000 });

    const scrollbar = await timeline.evaluate((element) => ({
      color: getComputedStyle(element).scrollbarColor,
      width: getComputedStyle(element, "::-webkit-scrollbar").width,
      buttonDisplay: getComputedStyle(element, "::-webkit-scrollbar-button").display,
      trackColor: getComputedStyle(element, "::-webkit-scrollbar-track").backgroundColor,
    }));
    expect(scrollbar.color).not.toBe("auto");
    expect(scrollbar.width).toBe("10px");
    expect(scrollbar.buttonDisplay).toBe("none");
    expect(scrollbar.trackColor).not.toBe("rgb(255, 255, 255)");
  });

  test("missing worktree rail shows a localized empty state without invalid actions", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/#conversation");
    await expect(page.getByTestId("conversation-rail")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("rail-tab-workspace").click();
    await expect(page.getByTestId("worktree-not-created")).toContainText(
      "隔离 Worktree 尚未创建",
    );
    await expect(page.getByTestId("conversation-rail")).not.toContainText(
      "Worktree is not registered",
    );
    await expect(page.getByRole("button", { name: "重新检查" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "对账" })).toHaveCount(0);
  });

  test("composer remains a compact dock when the optional workspace notice is absent", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.addInitScript(() => {
      window.localStorage.setItem("gag010:fixture-mode", "ask");
      window.localStorage.setItem("gag010:fixture-workspace", "direct");
    });
    await page.goto("/#conversation");
    const conversation = page.getByTestId("conversation-view");
    const composer = page.getByTestId("composer");
    const timeline = page.getByTestId("conversation-virtual-list");
    await expect(conversation).toBeVisible({ timeout: 15_000 });
    await expect(composer).toBeVisible();
    await expect(timeline).toBeVisible();
    await expect(page.getByTestId("conversation-workspace-notice")).toHaveCount(0);

    const [conversationBox, composerBox, timelineBox] = await Promise.all([
      conversation.boundingBox(),
      composer.boundingBox(),
      timeline.boundingBox(),
    ]);
    expect(conversationBox).not.toBeNull();
    expect(composerBox).not.toBeNull();
    expect(timelineBox).not.toBeNull();
    expect(composerBox!.height).toBeLessThanOrEqual(180);
    expect(timelineBox!.height).toBeGreaterThan(composerBox!.height);
  });

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
    await waitForConversationIdle(page);

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

    await waitForConversationIdle(page);

    const input = page.getByTestId("composer-input");
    await input.fill("new content while reading history");
    await page.getByTestId("composer-send").click();
    await expect(page.getByTestId("unread-count")).toHaveText(/^[1-9]\d*$/);
    expect(await list.evaluate((el) => el.scrollTop)).toBe(0);

    await jump.click();
    await expect(jump).toBeHidden({ timeout: 3000 });
  });

  test("long tool summaries never create page or timeline horizontal scrolling", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/#conversation");
    const timeline = page.getByTestId("conversation-virtual-list");
    await expect(timeline).toBeVisible({ timeout: 15_000 });
    const oneLine = page.locator(".tool-one-line").first();
    await expect(oneLine).toBeVisible({ timeout: 15_000 });

    const dimensions = await oneLine.evaluate((summary) => {
      const pageWidthBefore = document.documentElement.scrollWidth;
      summary.textContent = `{"content":"${"a/very-long-path-segment/".repeat(160)}"}`;
      const list = document.querySelector(
        '[data-testid="conversation-virtual-list"]',
      ) as HTMLElement | null;
      if (!list) throw new Error("conversation timeline not found");
      return {
        pageWidthBefore,
        pageScrollWidth: document.documentElement.scrollWidth,
        listClientWidth: list.clientWidth,
        listScrollWidth: list.scrollWidth,
      };
    });

    expect(dimensions.pageScrollWidth).toBeLessThanOrEqual(dimensions.pageWidthBefore);
    expect(dimensions.listScrollWidth).toBeLessThanOrEqual(dimensions.listClientWidth);
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
    // Scroll restoration from localStorage is async after reload;
    // jump-to-bottom only appears once the scroll position is restored.
    await page.waitForTimeout(1000);
    await expect(page.getByTestId("jump-to-bottom")).toBeVisible({ timeout: 10_000 });
    await expect
      .poll(() => restored.evaluate((el) => el.scrollTop), { timeout: 10_000 })
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
