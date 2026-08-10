import { mkdir, writeFile } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const npmCli = process.env.npm_execpath;
if (!npmCli) throw new Error("Run this gate through npm so npm_execpath is available.");
const npm = process.execPath;
const npmArgs = (...args) => [npmCli, ...args];
const cargo = process.platform === "win32" ? "cargo.exe" : "cargo";
const steps = [
  ["TypeScript typecheck", npm, npmArgs("run", "typecheck")],
  ["Renderer lint", npm, npmArgs("run", "lint")],
  ["Node and contract tests", npm, npmArgs("run", "test:node")],
  ["Static security and privacy gate", npm, npmArgs("run", "test:security")],
  ["Vue unit/component/performance tests", npm, npmArgs("run", "test:ui")],
  ["Playwright E2E", npm, npmArgs("run", "test:e2e")],
  ["Rust format", cargo, ["fmt", "--check", "--manifest-path", "src-tauri/Cargo.toml"]],
  ["Rust check", cargo, ["check", "--manifest-path", "src-tauri/Cargo.toml"]],
  ["Rust clippy", cargo, ["clippy", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets", "--", "-D", "warnings"]],
  ["Rust tests", cargo, ["test", "--manifest-path", "src-tauri/Cargo.toml"]],
  ["Frontend production build", npm, npmArgs("run", "build")],
  ["Tauri release candidate build", npm, npmArgs("run", "tauri", "build")],
];

function version(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", shell: false });
  return (result.stdout || result.stderr || "unavailable").trim();
}

function run(command, args) {
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(command, args, { cwd: process.cwd(), stdio: "inherit", shell: false });
    } catch (error) {
      resolve({ exitCode: null, error: error instanceof Error ? error.message : String(error) });
      return;
    }
    child.on("error", (error) => resolve({ exitCode: null, error: error.message }));
    child.on("exit", (exitCode) => resolve({ exitCode, error: null }));
  });
}

const report = {
  taskId: "GAG-015",
  startedAt: new Date().toISOString(),
  environment: {
    platform: `${process.platform}-${process.arch}`,
    node: process.version,
    npm: version(npm, npmArgs("--version")),
    cargo: version(cargo, ["--version"]),
    rustc: version(process.platform === "win32" ? "rustc.exe" : "rustc", ["--version"]),
    git: version(process.platform === "win32" ? "git.exe" : "git", ["--version"]),
    cpu: os.cpus()[0]?.model ?? "unknown",
    logicalCores: os.cpus().length,
    totalMemoryBytes: os.totalmem(),
    buildModes: ["Vite production", "Rust dev tests", "Tauri release candidate"],
  },
  steps: [],
};

for (const [name, command, args] of steps) {
  const started = Date.now();
  console.log(`\n[GAG-015] ${name}`);
  const result = await run(command, args);
  report.steps.push({ name, command, args, durationMs: Date.now() - started, ...result });
  if (result.exitCode !== 0) break;
}

report.finishedAt = new Date().toISOString();
report.status = report.steps.length === steps.length && report.steps.every((step) => step.exitCode === 0)
  ? "passed"
  : "failed";
const directory = path.join(process.cwd(), ".gag-015-evidence");
await mkdir(directory, { recursive: true });
await writeFile(path.join(directory, "release-gate.json"), `${JSON.stringify(report, null, 2)}\n`, "utf8");
console.log(`\nGAG-015 release gate: ${report.status}`);
if (report.status !== "passed") process.exitCode = 1;
