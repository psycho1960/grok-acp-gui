import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import test from "node:test";
import { createSSRApp } from "vue";
import { renderToString } from "@vue/server-renderer";
import { createServer } from "vite";

const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const readmeSource = await readFile("README.md", "utf8");
const agentsSource = await readFile("AGENTS.md", "utf8");
const gitignoreSource = await readFile(".gitignore", "utf8");
const tauriConfig = JSON.parse(
  await readFile("src-tauri/tauri.conf.json", "utf8"),
);
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const licenseText = await readFile("LICENSE", "utf8");
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
});

test("GAG-001 locks paired Tauri packages on matching minor versions", async () => {
  await access("src-tauri/Cargo.lock");
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

test("GAG-001 exports only the bootstrap bridge and fails closed", async () => {
  const bridge = await import("../src/bridge/desktop-bridge.ts");

  assert.deepEqual(Object.keys(bridge), ["bootstrap"]);
  await assert.rejects(bridge.bootstrap(), /Windows Tauri host/);
});

test("GAG-001 renders the onboarding placeholder without fake checks", async () => {
  const vite = await createServer({
    appType: "custom",
    logLevel: "silent",
    server: { middlewareMode: true },
  });
  try {
    const { default: OnboardingView } = await vite.ssrLoadModule(
      "/src/features/onboarding/OnboardingView.vue",
    );
    const html = await renderToString(createSSRApp(OnboardingView));

    assert.match(html, /UI-ONBOARD-001/);
    assert.match(html, /启动检查尚未接入/);
    assert.doesNotMatch(html, /选择项目目录|UI-PROJECT-001/);
  } finally {
    await vite.close();
  }
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

test("GAG-001 keeps both MIT copyright notices", () => {
  assert.match(licenseText, /Copyright \(c\) 2026 Jun Han/);
  assert.match(licenseText, /Copyright \(c\) 2026 Hon_Y/);
});

test("GAG-001 ignores common secret file formats", () => {
  const ignored = new Set(gitignoreSource.split(/\r?\n/));

  for (const pattern of ["*.pem", "*.key", "*.p12", "*.pfx"]) {
    assert.equal(ignored.has(pattern), true, pattern);
  }
});

test("AGENTS indexes the roadmap and task specifications", () => {
  assert.match(agentsSource, /docs\/04-AI-DEVELOPMENT-ROADMAP\.md/);
  assert.match(agentsSource, /docs\/tasks\//);
});

test("GAG-001 exposes the composition roots and complete CI gates", async () => {
  await access("src/app/bootstrap.ts");
  await access("src/features/onboarding/OnboardingView.vue");
  await access("src-tauri/src/app.rs");
  for (const command of ["cargo clippy", "cargo test", "npm run tauri build"]) {
    assert.match(ciSource, new RegExp(command.replace(/[.*+?^${}()|[\\]\\]/g, "\\\\$&")));
  }
});
