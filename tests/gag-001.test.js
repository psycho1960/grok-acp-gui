import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { access } from "node:fs/promises";
import test from "node:test";

const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const tauriConfig = JSON.parse(
  await readFile("src-tauri/tauri.conf.json", "utf8"),
);

test("GAG-001 exposes the desktop verification scripts", () => {
  for (const script of ["typecheck", "lint", "test", "build", "tauri"]) {
    assert.equal(typeof packageJson.scripts[script], "string", script);
  }
  assert.equal(packageJson.scripts["build:web"], undefined);
  assert.equal(packageJson.scripts["dev:web"], undefined);
});

test("GAG-001 metadata is Grok ACP GUI and Windows-only", async () => {
  assert.equal(packageJson.name, "grok-acp-gui");
  assert.equal(tauriConfig.productName, "Grok ACP GUI");
  assert.equal(tauriConfig.identifier, "com.grokacpgui.desktop");
  assert.deepEqual(tauriConfig.bundle.targets, ["nsis", "msi"]);
  assert.deepEqual(tauriConfig.app.windows[0].title, "Grok ACP GUI");
  await assert.rejects(
    access(["src-tauri", "capabilities", "mobile.json"].join("/")),
  );
});

test("GAG-001 removes web transport and telemetry dependencies", () => {
  assert.equal(
    Object.keys(packageJson.dependencies).some((name) =>
      name.toLowerCase().includes("application"),
    ),
    false,
  );
  assert.equal(packageJson.dependencies["vue-router"], undefined);
  assert.equal(packageJson.dependencies["marked"], undefined);
});
