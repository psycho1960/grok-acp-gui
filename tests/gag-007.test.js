import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

const featureRoot = "src/features/task-center";
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

test("GAG-007 provides Task Center feature modules", () => {
  for (const required of [
    "TaskCenterView.vue",
    "TaskCard.vue",
    "TaskDetailDrawer.vue",
    "VirtualList.vue",
    "task-center-store.ts",
    "task-bridge-facade.ts",
    "grouping.ts",
    "status-map.ts",
    "hash-route.ts",
    "list-rows.ts",
    "OpenProjectDialog.vue",
    "CreateTaskDialog.vue",
    "stateful-fake-bridge.ts",
    "index.ts",
  ]) {
    assert.ok(
      files.some((f) => f.replace(/\\/g, "/").endsWith(required)),
      `missing ${required}`,
    );
  }
});

test("GAG-007 facade maps onto existing DesktopBridge commands/events", () => {
  const facade = src("task-bridge-facade.ts");
  assert.match(facade, /task\.open/);
  assert.match(facade, /turn\.cancel/);
  assert.match(facade, /task\.snapshot/);
  assert.match(facade, /task\.state/);
  assert.match(facade, /project\.open/);
  assert.match(facade, /task\.create/);
  assert.match(facade, /workspace\.inspect/);
  assert.doesNotMatch(facade, /list_tasks/);
  assert.doesNotMatch(facade, /cancel_task/);
  assert.doesNotMatch(facade, /task:status_changed/);
});

test("GAG-007 closed-loop surfaces open project and create task CTAs", () => {
  const view = src("TaskCenterView.vue");
  assert.match(view, /header-open-project|empty-open-project/);
  assert.match(view, /header-create-task|empty-create-task/);
  assert.match(view, /#conversation\//);
});

test("GAG-007 does not hardcode Mocha palette hex in feature sources", () => {
  const palette =
    /#(?:11111b|181825|1e1e2e|313244|45475a|585b70|6c7086|a6adc8|cdd6f4|cba6f7|89b4fa|a6e3a1|f9e2af|f38ba8|fab387)/i;
  for (const [name, source] of Object.entries(sources)) {
    assert.doesNotMatch(source, palette, name);
  }
});

test("GAG-007 does not use v-html for untrusted task text", () => {
  for (const [name, source] of Object.entries(sources)) {
    if (!name.endsWith(".vue")) continue;
    assert.doesNotMatch(source, /v-html/, name);
  }
});

test("GAG-007 store keeps version/seq guards and non-optimistic cancel", () => {
  const store = src("task-center-store.ts");
  assert.match(store, /version/);
  assert.match(store, /stale/);
  assert.match(store, /cancelTask/);
  assert.match(store, /detailRequestId/);
  assert.match(store, /markStale/);
});

test("GAG-007 deep link helpers use hash task-center routes with group", async () => {
  const hash = src("hash-route.ts");
  assert.match(hash, /#task-center/);
  assert.match(hash, /group/);
  const app = await readFile("src/App.vue", "utf8");
  assert.match(app, /task-center|TaskCenterFixture/);
  assert.match(app, /UI-ERROR-001|dbUnavailable/);
});
