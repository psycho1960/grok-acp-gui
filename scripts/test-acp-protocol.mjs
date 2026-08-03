// Manual ACP protocol test — sends initialize and prints the raw response.
import { spawn } from "node:child_process";

const child = spawn("grok", ["--no-auto-update", "agent", "stdio"], {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env },
});

let stdoutData = "";
let stderrData = "";

child.stdout.on("data", (data) => {
  stdoutData += data.toString();
  process.stdout.write("[STDOUT] " + data.toString());
});

child.stderr.on("data", (data) => {
  stderrData += data.toString();
  process.stderr.write("[STDERR] " + data.toString());
});

child.on("exit", (code, signal) => {
  console.log("[EXIT] code=" + code + " signal=" + signal);
  console.log("[BUFFERS] stdout=" + stdoutData.length + " chars, stderr=" + stderrData.length + " chars");
});

// Send initialize after 1 second.
setTimeout(() => {
  const init = JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: 1,
      clientCapabilities: { fs: {}, terminal: {} },
    },
  });
  console.log("[SEND] " + init);
  child.stdin.write(init + "\n");
}, 1000);

// Close stdin after 8 seconds to let Grok exit.
setTimeout(() => {
  console.log("[CLOSE STDIN]");
  child.stdin.end();
}, 8000);

// Force exit after 12 seconds.
setTimeout(() => {
  console.log("[FORCE EXIT]");
  process.exit(0);
}, 12000);
