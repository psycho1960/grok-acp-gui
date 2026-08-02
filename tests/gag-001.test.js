import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { access } from "node:fs/promises";
import test from "node:test";

const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const tauriConfig = JSON.parse(
  await readFile("src-tauri/tauri.conf.json", "utf8"),
);
const licenseText = await readFile("LICENSE", "utf8");
const bridgeSource = await readFile("src/bridge/desktop-bridge.ts", "utf8");
const ciSource = await readFile(".github/workflows/ci.yml", "utf8");

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
  assert.deepEqual(tauriConfig.bundle.icon, ["icons/icon.ico"]);
  assert.deepEqual(tauriConfig.app.windows[0].title, "Grok ACP GUI");
  await assert.rejects(
    access(["src-tauri", "capabilities", "mobile.json"].join("/")),
  );
  await assert.rejects(access("assets/screenshot.png"));
  await access(["src-tauri", "icons", "icon.ico"].join("/"));
  await access(["src", "lib", "transport", "stdio.ts"].join("/"));
});

test("GAG-001 uses shared design tokens for onboarding focus", async () => {
  const source = await readFile("src/features/onboarding/OnboardingView.vue", "utf8");

  assert.match(source, /var\(--ctp-focus-ring\)/);
  assert.doesNotMatch(source, /rgb\(203 166 247/);
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

test("GAG-001 keeps both MIT copyright notices and fails closed outside Tauri", () => {
  assert.match(licenseText, /Copyright \(c\) 2026 Jun Han/);
  assert.match(licenseText, /Copyright \(c\) 2026 Hon_Y/);
  assert.doesNotMatch(bridgeSource, new RegExp(["local", "Storage"].join("")));
  assert.doesNotMatch(bridgeSource, /fallback/);
  assert.doesNotMatch(bridgeSource, /loadPreferences|savePreferences/);
});

test("GAG-001 exposes the composition roots and complete CI gates", async () => {
  await access("src/app/bootstrap.ts");
  await access("src/features/onboarding/OnboardingView.vue");
  await access("src-tauri/src/app.rs");
  for (const command of ["cargo clippy", "cargo test", "npm run tauri build"]) {
    assert.match(ciSource, new RegExp(command.replace(/[.*+?^${}()|[\\]\\]/g, "\\\\$&")));
  }
});
