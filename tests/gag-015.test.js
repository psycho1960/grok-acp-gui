import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

async function text(path) {
  return readFile(new URL(`../${path}`, import.meta.url), "utf8");
}

test("GAG-015 exposes deterministic release gates without retries", async () => {
  const pkg = JSON.parse(await text("package.json"));
  const playwright = await text("playwright.config.ts");
  assert.match(pkg.scripts["gate:gag015"], /gag-015-release-gate/);
  assert.match(pkg.scripts.test, /test:security/);
  assert.match(playwright, /retries:\s*0/);
  assert.doesNotMatch(playwright, /retries:\s*[1-9]/);
});

test("GAG-015 final traceability matrix covers every requirement family", async () => {
  const matrix = await text("docs/testing/GAG-015-traceability-matrix.md");
  for (const family of [
    "FR-RUNTIME", "FR-PROJECT", "FR-TASK", "FR-SESSION", "FR-PERMISSION",
    "FR-PLAN", "FR-IMAGE", "FR-WORKTREE", "FR-REVIEW", "FR-RECOVERY",
    "NFR-SECURITY", "NFR-PERFORMANCE", "NFR-RELIABILITY",
    "NFR-ACCESSIBILITY", "NFR-PRIVACY",
  ]) assert.match(matrix, new RegExp(family));
  assert.match(matrix, /证据负责人/);
});

test("GAG-015 report records the missing predecessor delivery evidence honestly", async () => {
  const report = await text("docs/testing/GAG-015-release-candidate-report.md");
  assert.match(report, /GAG-001～014.*交付报告/);
  assert.match(report, /未满足/);
  assert.doesNotMatch(report, /双旗舰独立审查.*通过/);
});
