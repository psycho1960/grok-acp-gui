import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/e2e",
  outputDir: "test-results/playwright",
  // Concurrent Chromium launches remained nondeterministic under the full
  // Windows gate even after reducing nine workers to four. One worker gives
  // the suite a single browser lifecycle without retries or inflated timeouts.
  workers: 1,
  retries: 0,
  snapshotPathTemplate: "{testDir}/{testFilePath}-snapshots/{arg}{ext}",
  use: {
    baseURL: "http://127.0.0.1:1420",
    browserName: "chromium",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
  },
});
