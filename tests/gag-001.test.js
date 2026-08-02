import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { access } from "node:fs/promises";
import test from "node:test";

const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const readmeSource = await readFile("README.md", "utf8");
const tauriConfig = JSON.parse(
  await readFile("src-tauri/tauri.conf.json", "utf8"),
);
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const licenseText = await readFile("LICENSE", "utf8");
const bridgeSource = await readFile("src/bridge/desktop-bridge.ts", "utf8");
const appSource = await readFile("src/App.vue", "utf8");
const onboardingSource = await readFile(
  "src/features/onboarding/OnboardingView.vue",
  "utf8",
);
const themeSource = await readFile("src/shared/theme/tokens.css", "utf8");
const taskSpecSource = await readFile(
  "docs/tasks/GAG-001-project-bootstrap.md",
  "utf8",
);
const technicalDesignSource = await readFile(
  "docs/03-TECHNICAL-DESIGN.md",
  "utf8",
);
const provenanceAdrSource = await readFile(
  "docs/adr/ADR-0001-upstream-provenance-without-shared-ancestry.md",
  "utf8",
);
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

test("GAG-001 keeps paired Tauri packages on matching minor versions", () => {
  assert.equal(packageJson.dependencies["@tauri-apps/api"], "2.11.1");
  assert.equal(packageJson.dependencies["@tauri-apps/plugin-dialog"], "2.7.2");
  assert.equal(packageJson.dependencies["@tauri-apps/plugin-store"], "2.4.4");
  assert.match(cargoToml, /tauri = \{ version = "~2\.11\.0"/);
  assert.match(cargoToml, /tauri-plugin-dialog = "~2\.7\.0"/);
  assert.match(cargoToml, /tauri-plugin-store = "~2\.4\.0"/);
});

test("GAG-001 records accepted upstream provenance without ancestry", () => {
  assert.match(provenanceAdrSource, /状态：Accepted/);
  assert.match(readmeSource, /ADR-0001/);
  assert.match(taskSpecSource, /不以 Git ancestry 作为验收条件/);
  assert.match(technicalDesignSource, /不要求属于产品仓库的 Git 祖先链/);
});

test("GAG-001 exposes only the bootstrap bridge and onboarding placeholder", () => {
  assert.doesNotMatch(bridgeSource, /selectProjectDirectory/);
  assert.doesNotMatch(appSource, /selectProjectDirectory|projectPath/);
  assert.match(onboardingSource, /UI-ONBOARD-001/);
  assert.match(onboardingSource, /启动检查尚未接入/);
  assert.doesNotMatch(onboardingSource, /选择项目目录|UI-PROJECT-001/);
});

test("GAG-001 keeps shared visual knowledge in the theme", () => {
  assert.match(themeSource, /\.eyebrow\s*\{/);
  assert.doesNotMatch(appSource, /\.eyebrow\s*\{/);
  assert.doesNotMatch(onboardingSource, /\.eyebrow\s*\{/);
  assert.doesNotMatch(onboardingSource, /\.workspace-card\s*\{/);
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
