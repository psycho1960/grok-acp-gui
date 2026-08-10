import assert from "node:assert/strict";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const root = process.cwd();

async function filesUnder(relative, extensions) {
  const output = [];
  async function visit(current) {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const absolute = path.join(current, entry.name);
      if (entry.isDirectory()) await visit(absolute);
      else if (extensions.some((extension) => entry.name.endsWith(extension))) {
        output.push(absolute);
      }
    }
  }
  await visit(path.join(root, relative));
  return output;
}

function relative(absolute) {
  return path.relative(root, absolute).replaceAll("\\", "/");
}

function productionRust(source) {
  const testModule = source.search(/\n#\[cfg\(test\)\]/);
  return testModule < 0 ? source : source.slice(0, testModule);
}

const findings = [];
function reject(file, rule, pattern, source) {
  if (pattern.test(source)) findings.push({ file: relative(file), rule });
}

const rendererRoots = ["src/app", "src/features", "src/shared"];
for (const rendererRoot of rendererRoots) {
  for (const file of await filesUnder(rendererRoot, [".ts", ".vue"])) {
    const source = await readFile(file, "utf8");
    reject(file, "renderer-direct-tauri-invoke", /@tauri-apps\/api\/core|\binvoke\s*\(/, source);
    reject(file, "renderer-shell-api", /node:child_process|child_process|\bexecFile?\s*\(|\bspawn\s*\(/, source);
    reject(file, "active-embed", /<(?:iframe|object|embed)\b/i, source);
    if (/\bv-html\s*=/.test(source) && relative(file) !== "src/features/conversation/SafeMarkdown.vue") {
      findings.push({ file: relative(file), rule: "unapproved-v-html" });
    }
  }
}

const safeMarkdownPath = path.join(root, "src/features/conversation/SafeMarkdown.vue");
const safeMarkdown = await readFile(safeMarkdownPath, "utf8");
assert.match(safeMarkdown, /renderSafeMarkdown/);
assert.match(safeMarkdown, /v-html="html"/);

for (const file of await filesUnder("src-tauri/src", [".rs"])) {
  const source = productionRust(await readFile(file, "utf8"));
  reject(file, "shell-command-string", /Command::new\(\s*"(?:cmd(?:\.exe)?|powershell(?:\.exe)?|pwsh|sh|bash)"\s*\)/i, source);
  reject(file, "shell-control-argument", /\.arg\(\s*"(?:\/C|-Command|-c)"\s*\)/i, source);
  if (relative(file).startsWith("src-tauri/src/adapters/grok_acp/") && /\b(?:println!|print!)\s*\(/.test(source)) {
    findings.push({ file: relative(file), rule: "acp-stdout-log" });
  }
}

const capabilityPath = path.join(root, "src-tauri/capabilities/default.json");
const capability = JSON.parse(await readFile(capabilityPath, "utf8"));
const forbiddenPermissions = /(?:shell|process|fs:|http:|sql|clipboard:allow-read)/i;
for (const permission of capability.permissions ?? []) {
  if (forbiddenPermissions.test(permission)) {
    findings.push({ file: relative(capabilityPath), rule: `forbidden-capability:${permission}` });
  }
}

const tauriConfigPath = path.join(root, "src-tauri/tauri.conf.json");
const tauriConfig = JSON.parse(await readFile(tauriConfigPath, "utf8"));
const csp = tauriConfig?.app?.security?.csp ?? "";
assert.match(csp, /default-src 'self'/);
assert.match(csp, /connect-src 'self' ipc: http:\/\/ipc\.localhost/);
assert.doesNotMatch(csp, /script-src[^;]*https?:/);
assert.doesNotMatch(csp, /object-src[^;]*(?:https?:|\*)/);

const productionFiles = [
  ...(await filesUnder("src", [".ts", ".vue"])),
  ...(await filesUnder("src-tauri/src", [".rs"])),
];
const credentialPatterns = [
  /\bxai-[A-Za-z0-9_-]{16,}\b/,
  /\bsk-[A-Za-z0-9_-]{20,}\b/,
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/,
];
for (const file of productionFiles) {
  const rawSource = await readFile(file, "utf8");
  const source = file.endsWith(".rs") ? productionRust(rawSource) : rawSource;
  for (const pattern of credentialPatterns) reject(file, "credential-literal", pattern, source);
}

const evidence = {
  taskId: "GAG-015",
  generatedAt: new Date().toISOString(),
  checks: {
    rendererFiles: rendererRoots,
    rustSource: "src-tauri/src (test-only modules excluded)",
    capability: relative(capabilityPath),
    csp: relative(tauriConfigPath),
    credentialPatterns: credentialPatterns.length,
  },
  findings,
  status: findings.length === 0 ? "passed" : "failed",
};

const evidenceDirectory = path.join(root, ".gag-015-evidence");
await mkdir(evidenceDirectory, { recursive: true });
await writeFile(
  path.join(evidenceDirectory, "static-gate.json"),
  `${JSON.stringify(evidence, null, 2)}\n`,
  "utf8",
);

if (findings.length > 0) {
  console.error(JSON.stringify(evidence, null, 2));
  process.exitCode = 1;
} else {
  console.log(`GAG-015 static gate passed (${productionFiles.length} production files scanned).`);
}
