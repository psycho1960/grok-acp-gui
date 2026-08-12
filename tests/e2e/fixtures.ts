import { test as base, expect } from "@playwright/test";

/** Keep in sync with FirstUseCoach.vue FIRST_USE_COACH_STORAGE_KEY. */
const FIRST_USE_COACH_STORAGE_KEY = "gag-ui-first-use-coach-v1";

/**
 * Default e2e context dismisses the first-use coach so product shells
 * (gag-002, closed-loop, etc.) exercise core flows without a blocking overlay.
 * Coach UI is still testable by clearing the key or navigating to `#coach`.
 */
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
