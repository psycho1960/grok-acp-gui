// Probe: can we choose a model for the real Grok ACP session?
// Usage: node scripts/test-acp-model.mjs [modelId]
// Defaults to grok-4.5 (xAI channel) to test bypassing the DeepSeek 401.
import { spawn } from "node:child_process";
import { Writable, Readable } from "node:stream";
import * as acp from "@agentclientprotocol/sdk/dist/acp.js";

const modelId = process.argv[2] || "grok-4.5";
console.log(`[INFO] starting grok with --model ${modelId}`);

const args = ["--no-auto-update", "agent", "--model", modelId, "stdio"];
const child = spawn("grok", args, {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env },
});

let stderrTail = [];
child.stderr.on("data", (data) => {
  const lines = data.toString().split("\n").filter(Boolean);
  for (const l of lines) stderrTail.push(l);
  if (stderrTail.length > 30) stderrTail = stderrTail.slice(-30);
});

const client = {
  async sessionUpdate(params) {
    const u = params.update;
    const kind = u && u.sessionUpdate ? u.sessionUpdate : "(unknown)";
    if (kind === "agent_message_chunk" && u.content?.type === "text") {
      process.stdout.write(u.content.text);
    } else if (kind === "models_update" || (u && u.currentModelId)) {
      console.log(`\n[EVENT] models_update: currentModelId=${u.currentModelId}`);
    } else if (kind === "turn_completed" || kind === "session_ready" || kind === "available_commands_update") {
      console.log(`[EVENT] ${kind}${u.stop_reason ? " stop_reason=" + u.stop_reason : ""}`);
    }
    return {};
  },
  async sessionRequestPermission(params) {
    console.log(`[PERMISSION] ${params.toolCall?.title}`);
    return { outcome: { outcome: "selected", optionId: params.options[0].optionId } };
  },
  async fsReadTextFile() { return { content: "" }; },
  async fsWriteTextFile() { return {}; },
};

const input = Writable.toWeb(child.stdin);
const output = Readable.toWeb(child.stdout);
const stream = acp.ndJsonStream(input, output);
const connection = new acp.ClientSideConnection(() => client, stream);

let done = false;
child.on("exit", (code, signal) => {
  if (!done) { console.log(`\n[EXIT] code=${code} signal=${signal}`); }
});

try {
  const init = await connection.initialize({
    protocolVersion: acp.PROTOCOL_VERSION,
    clientCapabilities: { fs: { readTextFile: true, writeTextFile: true } },
  });
  console.log(`[INIT OK] protocol=${init.protocolVersion} agent=${init.agentName} v${init.agentVersion}`);

  const session = await connection.newSession({ cwd: process.cwd(), mcpServers: [] });
  console.log(`[SESSION OK] id=${session.sessionId}`);

  // Wait a moment for models_update notifications, then check what model is active.
  await new Promise((r) => setTimeout(r, 3000));

  console.log(`[PROMPT] sending minimal request (expecting model ${modelId})...`);
  const promptPromise = connection.prompt({
    sessionId: session.sessionId,
    prompt: [{ type: "text", text: "Reply with exactly one word: PONG" }],
  });
  const timeout = new Promise((_, rej) => setTimeout(() => rej(new Error("prompt timeout")), 60000));
  try {
    const r = await Promise.race([promptPromise, timeout]);
    console.log(`\n[PROMPT OK] stopReason=${r.stopReason}`);
  } catch (e) {
    console.log(`\n[PROMPT FAIL] ${e.message}`);
  }

  await new Promise((r) => setTimeout(r, 2000));
} catch (e) {
  console.log(`[FATAL] ${e.message}`);
} finally {
  done = true;
  try { child.kill("SIGKILL"); } catch { /* process already gone */ }
  console.log("\n[STDERR TAIL (sanitized)]");
  for (const line of stderrTail.slice(-12)) {
    const s = line
      .replace(/Bearer [A-Za-z0-9._-]+/g, "Bearer ****")
      .replace(/api[_-]?key["'=:\s]+[A-Za-z0-9._-]+/gi, "apikey ****")
      .replace(/sk-[A-Za-z0-9]+/g, "sk-****");
    console.log("  " + s.slice(0, 240));
  }
  process.exit(0);
}
