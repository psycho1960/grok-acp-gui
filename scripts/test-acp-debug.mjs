// Debug test: send initialize with --debug flag to see Grok internals.
import { spawn } from "node:child_process";

const child = spawn("grok", ["--no-auto-update", "--debug", "agent", "stdio"], {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env, GROK_LOG: "debug" },
});

let stdoutLines = [];
let stderrLines = [];

child.stdout.on("data", (data) => {
  const text = data.toString();
  stdoutLines.push(text);
  process.stdout.write("[STDOUT] " + text);
});

child.stderr.on("data", (data) => {
  const text = data.toString();
  stderrLines.push(text);
  // Only print first 2000 chars of stderr to avoid flood
  if (stderrLines.length <= 50) {
    process.stderr.write("[STDERR] " + text);
  }
});

child.on("exit", (code, signal) => {
  console.log("\n[EXIT] code=" + code + " signal=" + signal);
  console.log("[STDOUT lines] " + stdoutLines.length);
  console.log("[STDERR lines] " + stderrLines.length);
});

// Send initialize after 2 seconds.
setTimeout(() => {
  const init = JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: 1,
      clientCapabilities: {
        fs: { readTextFile: true, writeTextFile: true },
      },
    },
  });
  console.log("[SEND] " + init);
  child.stdin.write(init + "\n");
}, 2000);

// Send session/new after 5 seconds.
setTimeout(() => {
  const newSession = JSON.stringify({
    jsonrpc: "2.0",
    id: 2,
    method: "session/new",
    params: {
      cwd: process.cwd(),
      mcpServers: [],
    },
  });
  console.log("[SEND] " + newSession);
  child.stdin.write(newSession + "\n");
}, 5000);

// Close stdin after 10 seconds.
setTimeout(() => {
  console.log("[CLOSE STDIN]");
  child.stdin.end();
}, 10000);

// Force exit after 15 seconds.
setTimeout(() => {
  console.log("[FORCE EXIT]");
  child.kill("SIGTERM");
  process.exit(0);
}, 15000);
