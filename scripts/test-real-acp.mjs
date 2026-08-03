// Test real Grok ACP using the official SDK.
import { spawn } from "node:child_process";
import { Writable, Readable } from "node:stream";
import * as acp from "@agentclientprotocol/sdk/dist/acp.js";

const child = spawn("grok", ["--no-auto-update", "agent", "stdio"], {
  stdio: ["pipe", "pipe", "pipe"],
  env: { ...process.env },
});

let stderrData = "";
child.stderr.on("data", (data) => {
  stderrData += data.toString();
  process.stderr.write("[STDERR] " + data.toString());
});

child.on("exit", (code, signal) => {
  console.log("[EXIT] code=" + code + " signal=" + signal);
});

const input = Writable.toWeb(child.stdin);
const output = Readable.toWeb(child.stdout);

// Minimal client implementation
const client = {
  async session_update(params) {
    const update = params.update;
    console.log("[UPDATE] " + update.sessionUpdate);
    if (update.sessionUpdate === "agent_message_chunk" && update.content?.type === "text") {
      process.stdout.write(update.content.text);
    }
    return {};
  },
  async session_request_permission(params) {
    console.log("[PERMISSION] " + params.toolCall?.title);
    return { outcome: { outcome: "selected", optionId: params.options[0].optionId } };
  },
  async fs_read_text_file(params) {
    console.log("[FS_READ] " + params.path);
    return { content: "" };
  },
  async fs_write_text_file(params) {
    console.log("[FS_WRITE] " + params.path);
    return {};
  },
};

const stream = acp.ndJsonStream(input, output);
const connection = new acp.ClientSideConnection(() => client, stream);

try {
  console.log("[INIT] Sending initialize...");
  const initResult = await connection.initialize({
    protocolVersion: acp.PROTOCOL_VERSION,
    clientCapabilities: {
      fs: { readTextFile: true, writeTextFile: true },
    },
  });
  console.log("[INIT OK] protocol=" + initResult.protocolVersion + " agent=" + initResult.agentName + " v" + initResult.agentVersion);
  if (initResult.instructions) console.log("[INIT] instructions: " + initResult.instructions.substring(0, 100));

  console.log("[SESSION] Creating new session...");
  const sessionResult = await connection.newSession({
    cwd: process.cwd(),
    mcpServers: [],
  });
  console.log("[SESSION OK] sessionId=" + sessionResult.sessionId);

  console.log("[PROMPT] Sending prompt...");
  const promptResult = await connection.prompt({
    sessionId: sessionResult.sessionId,
    prompt: [{ type: "text", text: "Reply with exactly one word: PONG" }],
  });
  console.log("\n[PROMPT OK] stopReason=" + promptResult.stopReason);
} catch (error) {
  console.error("[ERROR] " + error.message);
  if (stderrData) {
    console.error("[STDERR SNAPSHOT]\n" + stderrData.substring(0, 500));
  }
} finally {
  child.kill();
  process.exit(0);
}
