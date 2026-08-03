// GAG-005 real Grok verification: handshake -> minimal request -> structured
// events -> abnormal exit. Uses the official ACP SDK against the REAL grok
// executable on this machine.
//
// Run:  node scripts/test-real-acp-abnormal-exit.mjs
// Exit codes: 0 = protocol+exit handling verified, 1 = failure.
import { spawn } from "node:child_process";
import { Writable, Readable } from "node:stream";
import * as acp from "@agentclientprotocol/sdk/dist/acp.js";

const results = [];
const updates = new Map(); // sessionUpdate kind -> count
let stderrTail = [];

function record(kind) {
  updates.set(kind, (updates.get(kind) || 0) + 1);
}
function logOk(name) {
  results.push(`PASS  ${name}`);
  console.log(`[PASS] ${name}`);
}
function logFail(name, detail) {
  results.push(`FAIL  ${name}: ${detail}`);
  console.log(`[FAIL] ${name}: ${detail}`);
}

const child = spawn("grok", ["--no-auto-update", "agent", "stdio"], {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env },
});

child.stderr.on("data", (data) => {
  const lines = data.toString().split("\n").filter(Boolean);
  for (const l of lines) stderrTail.push(l);
  if (stderrTail.length > 50) stderrTail = stderrTail.slice(-50);
  process.stderr.write("[STDERR] " + lines[0] + "\n");
});

// Correct SDK handler names (camelCase, per @agentclientprotocol/sdk).
const client = {
  async sessionUpdate(params) {
    const u = params.update;
    const kind = u && u.sessionUpdate ? u.sessionUpdate : "(unknown)";
    record(kind);
    if (kind === "agent_message_chunk" && u.content?.type === "text") {
      process.stdout.write(u.content.text);
    }
    return {};
  },
  async sessionRequestPermission(params) {
    record("permission_requested:" + (params.toolCall?.title || "?"));
    return { outcome: { outcome: "selected", optionId: params.options[0].optionId } };
  },
  async fsReadTextFile(params) {
    record("fs_read:" + params.path);
    return { content: "" };
  },
  async fsWriteTextFile(params) {
    record("fs_write:" + params.path);
    return {};
  },
};

const input = Writable.toWeb(child.stdin);
const output = Readable.toWeb(child.stdout);
const stream = acp.ndJsonStream(input, output);
const connection = new acp.ClientSideConnection(() => client, stream);

let exitInfo = null;
child.on("exit", (code, signal) => {
  exitInfo = { code, signal };
  console.log(`[EXIT] code=${code} signal=${signal}`);
});

try {
  // 1. Handshake
  const initResult = await connection.initialize({
    protocolVersion: acp.PROTOCOL_VERSION,
    clientCapabilities: { fs: { readTextFile: true, writeTextFile: true } },
  });
  if (initResult.protocolVersion === 1) logOk(`ACP handshake (protocol=${initResult.protocolVersion})`);
  else logFail("ACP handshake", `unexpected protocol ${initResult.protocolVersion}`);
  console.log(`[INFO] agent=${initResult.agentName} version=${initResult.agentVersion}`);

  // 2. Session
  const sessionResult = await connection.newSession({
    cwd: process.cwd(),
    mcpServers: [],
  });
  if (sessionResult.sessionId) logOk(`session/new (id=${sessionResult.sessionId})`);
  else logFail("session/new", "no session id");

  // 3. Minimal request
  const promptPromise = connection.prompt({
    sessionId: sessionResult.sessionId,
    prompt: [{ type: "text", text: "Reply with exactly one word: PONG" }],
  });
  const timeout = new Promise((_, rej) => setTimeout(() => rej(new Error("prompt timeout")), 60000));
  let promptResult;
  try {
    promptResult = await Promise.race([promptPromise, timeout]);
    logOk("prompt accepted, stopReason=" + promptResult.stopReason);
  } catch (e) {
    logFail("prompt", e.message);
  }

  // 4. Structured event summary (received so far)
  console.log("[EVENTS] received update kinds:");
  for (const [k, c] of [...updates.entries()].sort()) console.log(`  ${k} x${c}`);
  const hasStructured = updates.has("user_message_chunk") || updates.has("agent_message_chunk") ||
    updates.has("turn_completed") || updates.has("available_commands_update");
  if (hasStructured) logOk("structured events received");
  else logFail("structured events", "none observed");

  // 5. Abnormal exit: hard-kill the child (SIGTERM on Windows forces terminate).
  console.log("[KILL] sending SIGTERM (abnormal exit simulation)...");
  child.kill("SIGTERM");
  await new Promise((r) => setTimeout(r, 5000));
  if (exitInfo) {
    logOk(`process exited after kill (code=${exitInfo.code} signal=${exitInfo.signal})`);
  } else {
    console.log("[KILL] fallback: taskkill /F");
    const { execFileSync } = await import("node:child_process");
    try { execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"]); } catch { /* already exited */ }
    await new Promise((r) => setTimeout(r, 3000));
    if (exitInfo) logOk(`process exited after taskkill (code=${exitInfo.code} signal=${exitInfo.signal})`);
    else logFail("abnormal exit", "process did not exit");
  }
} catch (error) {
  logFail("fatal", error.message);
  console.error("[STDERR TAIL]\n" + stderrTail.slice(-15).join("\n"));
} finally {
  try { child.kill("SIGKILL"); } catch { /* process already gone */ }
  console.log("[STDERR TAIL (sanitized)]");
  for (const line of stderrTail.slice(-15)) {
    const s = line.replace(/Bearer [A-Za-z0-9._-]+/g, "Bearer ****")
      .replace(/(api[_-]?key["'=:\s]+)[A-Za-z0-9._-]+/gi, "$1****")
      .replace(/sk-[A-Za-z0-9]+/g, "sk-****");
    console.log("  " + s.slice(0, 220));
  }
  const failed = results.filter((r) => r.startsWith("FAIL"));
  console.log(failed.length === 0 ? "[SUMMARY] ALL CHECKS PASSED" : `[SUMMARY] ${failed.length} CHECK(S) FAILED`);
  process.exit(failed.length === 0 ? 0 : 1);
}
