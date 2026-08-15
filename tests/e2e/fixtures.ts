import { test as base, expect, type Page } from "@playwright/test";

/** Keep in sync with FirstUseCoach.vue FIRST_USE_COACH_STORAGE_KEY. */
const FIRST_USE_COACH_STORAGE_KEY = "gag-ui-first-use-coach-v1";

/**
 * Default e2e context dismisses the first-use coach so product shells
 * (gag-002, closed-loop, etc.) exercise core flows without a blocking overlay.
 * Coach UI is still testable by clearing the key or navigating to `#coach`.
 */
/** Stop a running fixture turn and wait for the task-bar status, not a Badge. */
export async function waitForConversationIdle(page: Page): Promise<void> {
  const stop = page.getByTestId("composer-stop");
  if (await stop.isVisible().catch(() => false)) {
    await stop.click();
  }
  await expect(page.getByTestId("conversation-status")).toContainText("空闲", {
    timeout: 10_000,
  });
}

export const test = base.extend({
  context: async ({ context }, use) => {
    await context.addInitScript((key: string) => {
      try {
        localStorage.setItem(key, "1");
      } catch {
        /* ignore */
      }
    }, FIRST_USE_COACH_STORAGE_KEY);
    await use(context);
  },
});

export { expect };
