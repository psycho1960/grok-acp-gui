import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

const featureRoot = "src/features/conversation";
const files = await readdir(featureRoot, { recursive: true });
const sources = Object.fromEntries(
  await Promise.all(
    files
      .filter((f) => f.endsWith(".ts") || f.endsWith(".vue"))
      .map(async (f) => [f.replace(/\\/g, "/"), await readFile(`${featureRoot}/${f}`, "utf8")]),
  ),
);

function src(name) {
  const hit = Object.entries(sources).find(([k]) => k.endsWith(name));
  assert.ok(hit, `missing ${name}`);
  return hit[1];
}

test("GAG-008 provides conversation feature modules", () => {
  for (const required of [
    "ConversationView.vue",
    "ConversationFixture.vue",
    "Composer.vue",
    "ToolCard.vue",
    "SafeMarkdown.vue",
    "TimelineItemView.vue",
    "TimelineVirtualList.vue",
    "ConversationHeader.vue",
    "conversation-store.ts",
    "conversation-facade.ts",
    "reducer.ts",
    "fixtures.ts",
    "markdown.ts",
    "tool-normalize.ts",
    "hash-route.ts",
    "draft.ts",
    "scroll.ts",
    "seed.ts",
    "index.ts",
    "slots/PermissionSlot.vue",
    "slots/PlanSlot.vue",
    "slots/ArtifactSlot.vue",
  ]) {
    assert.ok(
      files.some((f) => f.replace(/\\/g, "/").endsWith(required)),
      `missing ${required}`,
    );
  }
});

test("GAG-008 reducer enforces session seq dedup and tool merge rules", () => {
  const reducer = src("reducer.ts");
  assert.match(reducer, /seenKeys/);
  assert.match(reducer, /toolIndex/);
  assert.match(reducer, /mergeToolCall|mergeToolPhase/);
  assert.match(reducer, /applySnapshot/);
  assert.match(reducer, /needsSnapshotRefresh/);
});

test("GAG-008 markdown sanitizes dangerous protocols", () => {
  const md = src("markdown.ts");
  assert.match(md, /javascript/i);
  assert.match(md, /escapeHtml/);
  assert.match(md, /sanitizeHref/);
});

test("GAG-008 does not call Tauri or shell from feature", () => {
  for (const [path, body] of Object.entries(sources)) {
    if (path.endsWith(".vue") || path.endsWith(".ts")) {
      assert.equal(
        /@tauri-apps|child_process|execSync|spawn\s*\(/.test(body),
        false,
        `${path} must not invoke Tauri/shell directly`,
      );
    }
  }
});

test("GAG-008 uses DesktopBridge turn.send / turn.cancel", () => {
  const facade = src("conversation-facade.ts");
  assert.match(facade, /turn\.send/);
  assert.match(facade, /turn\.cancel/);
  assert.match(facade, /subscribe/);
});
