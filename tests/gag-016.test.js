import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const node = process.execPath;
const releaseScript = join(root, "scripts", "gag-016-release.mjs");

test("GAG-016 packaging configuration is internally consistent", () => {
  const output = execFileSync(node, [releaseScript, "verify"], { cwd: root, encoding: "utf8" });
  assert.match(output, /packaging configuration verified/);
});

test("GAG-016 manifest includes both installers and rejects missing MSI", () => {
  const fixture = mkdtempSync(join(tmpdir(), "gag-016-unicode 空格-"));
  const bundle = join(fixture, "bundle");
  const evidence = join(fixture, "evidence");
  mkdirSync(join(bundle, "nsis"), { recursive: true });
  mkdirSync(join(bundle, "msi"), { recursive: true });
  writeFileSync(join(bundle, "nsis", "Grok ACP GUI_0.1.16_x64-setup.exe"), "nsis-fixture");
  writeFileSync(join(bundle, "msi", "Grok ACP GUI_0.1.16_x64_en-US.msi"), "msi-fixture");

  try {
    execFileSync(node, [releaseScript, "manifest", bundle, evidence, "--allow-dirty"], { cwd: root, encoding: "utf8" });
    const manifest = JSON.parse(readFileSync(join(evidence, "artifact-manifest.json"), "utf8"));
    assert.ok(["clean", "dirty"].includes(manifest.sourceTreeState));
    assert.equal(
      manifest.candidateType,
      manifest.sourceTreeState === "dirty" ? "development-unsigned-candidate" : "internal-unsigned-candidate",
    );
    assert.equal(manifest.architecture, "x86_64-pc-windows-msvc");
    assert.deepEqual(manifest.artifacts.map((item) => item.file), [
      "msi/Grok ACP GUI_0.1.16_x64_en-US.msi",
      "nsis/Grok ACP GUI_0.1.16_x64-setup.exe",
    ]);
    assert.ok(manifest.artifacts.every((item) => /^[a-f0-9]{64}$/.test(item.sha256)));
    assert.match(readFileSync(join(evidence, "checksums.sha256"), "utf8"), /\.msi/);

    const signedFailure = spawnSync(node, [releaseScript, "manifest", bundle, evidence, "--require-signed", "--allow-dirty"], {
      cwd: root,
      encoding: "utf8",
    });
    assert.notEqual(signedFailure.status, 0);
    assert.match(signedFailure.stderr, /is not validly signed/);

    rmSync(join(bundle, "msi"), { recursive: true, force: true });
    const failure = spawnSync(node, [releaseScript, "manifest", bundle, evidence, "--allow-dirty"], { cwd: root, encoding: "utf8" });
    assert.notEqual(failure.status, 0);
    assert.match(failure.stderr, /MSI installer is missing/);
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});
