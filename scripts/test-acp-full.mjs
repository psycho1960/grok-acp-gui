// Comprehensive debug test with debug-file logging.
import { spawn } from "node:child_process";
import { writeFileSync, readFileSync, existsSync } from "node:fs";

const debugFile = "D:/codex/grok acp gui/grok-debug.log";
try { writeFileSync(debugFile, ""); } catch { /* debug log is best-effort */ }

const child = spawn("grok", ["agent", "stdio", "--debug-file", debugFile], {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env },
});

let stdoutBuf = Buffer.alloc(0);
let stderrBuf = Buffer.alloc(0);

child.stdout.on("data", (data) => {
  stdoutBuf = Buffer.concat([stdoutBuf, data]);
  console.log("[STDOUT] " + data.length + " bytes: " + data.toString().substring(0, 200));
});

child.stderr.on("data", (data) => {
  stderrBuf = Buffer.concat([stderrBuf, data]);
  console.log("[STDERR] " + data.length + " bytes: " + data.toString().substring(0, 200));
});

child.on("error", (err) => {
  console.log("[ERROR] " + err.message);
});

child.on("exit", (code, signal) => {
  console.log("[EXIT] code=" + code + " signal=" + signal);
  // Read debug log
  if (existsSync(debugFile)) {
    const log = readFileSync(debugFile, "utf8");
    console.log("[DEBUG LOG] " + log.length + " chars:");
    console.log(log.substring(0, 2000));
  }
});

// Wait 2 seconds, then send initialize.
setTimeout(() => {
  console.log("[SEND] initialize");
  const init = JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: true, writeTextFile: true } },
    },
  }) + "\n";
  child.stdin.write(init);
}, 2000);

// Wait 5 more seconds, then send session/new.
setTimeout(() => {
  console.log("[SEND] session/new");
  const newSession = JSON.stringify({
    jsonrpc: "2.0",
    id: 2,
    method: "session/new",
    params: { cwd: "D:/codex/grok acp gui", mcpServers: [] },
  }) + "\n";
  child.stdin.write(newSession);
}, 7000);

// Wait 10 seconds total, then kill.
setTimeout(() => {
  console.log("[KILL]");
  child.kill("SIGTERM");
}, 12000);

// Force exit after 15 seconds.
setTimeout(() => {
  console.log("[FORCE EXIT]");
  process.exit(1);
}, 15000);
