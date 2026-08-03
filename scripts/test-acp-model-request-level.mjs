// Verify request-level model override: does Grok accept `model` in session/prompt?
// Start with default model, then prompt with model=grok-4.5 and observe currentModelId.
import { spawn } from "node:child_process";
import { Writable, Readable } from "node:stream";
import * as acp from "@agentclientprotocol/sdk/dist/acp.js";

const child = spawn("grok", ["--no-auto-update", "agent", "stdio"], {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env },
});

let stderrTail = [];
child.stderr.on("data", (d) => {
  const lines = d.toString().split("\n").filter(Boolean);
  for (const l of lines) stderrTail.push(l);
  if (stderrTail.length > 30) stderrTail = stderrTail.slice(-30);
});

let modelEvents = [];
const client = {
  async sessionUpdate(params) {
    const u = params.update;
    const kind = u && u.sessionUpdate ? u.sessionUpdate : "(unknown)";
    if (u && u.currentModelId) modelEvents.push(u.currentModelId);
    if (kind === "agent_message_chunk" && u.content?.type === "text") process.stdout.write(u.content.text);
    return {};
  },
  async sessionRequestPermission(params) {
    return { outcome: { outcome: "selected", optionId: params.options[0].optionId } };
  },
  async fsReadTextFile() { return { content: "" }; },
  async fsWriteTextFile() { return {}; },
};

const stream = acp.ndJsonStream(Writable.toWeb(child.stdin), Readable.toWeb(child.stdout));
const connection = new acp.ClientSideConnection(() => client, stream);
let done = false;
child.on("exit", (code, signal) => { if (!done) console.log(`\n[EXIT] code=${code} signal=${signal}`); });

try {
  await connection.initialize({
    protocolVersion: acp.PROTOCOL_VERSION,
    clientCapabilities: { fs: { readTextFile: true, writeTextFile: true } },
  });
  const session = await connection.newSession({ cwd: process.cwd(), mcpServers: [] });
  console.log(`[SESSION OK] id=${session.sessionId}`);
  await new Promise((r) => setTimeout(r, 2500));
  console.log(`[BEFORE] currentModelId seen: ${[...new Set(modelEvents)].join(", ")}`);

  console.log("[PROMPT] with model=grok-4.5 in prompt params...");
  // The SDK prompt() may not accept model; use raw request via sendRequest if available.
  const promptPromise = connection.prompt({
    sessionId: session.sessionId,
    prompt: [{ type: "text", text: "Reply with exactly one word: PONG" }],
    model: "grok-4.5", // attempt request-level override
  });
  const timeout = new Promise((_, rej) => setTimeout(() => rej(new Error("prompt timeout")), 60000));
  try {
    const r = await Promise.race([promptPromise, timeout]);
    console.log(`\n[PROMPT OK] stopReason=${r.stopReason}`);
  } catch (e) {
    console.log(`\n[PROMPT FAIL] ${e.message}`);
  }
  await new Promise((r) => setTimeout(r, 1500));
  console.log(`[AFTER] currentModelId seen: ${[...new Set(modelEvents)].join(", ")}`);
} catch (e) {
  console.log(`[FATAL] ${e.message}`);
} finally {
  done = true;
  try { child.kill("SIGKILL"); } catch { /* process already gone */ }
  process.exit(0);
}
