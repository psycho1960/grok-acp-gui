// Repro: full ACP turn (initialize -> authenticate -> session/new -> prompt)
// with configurable --model and --strip-env, mirroring the app's spawn.
import { spawn } from "node:child_process";
import readline from "node:readline";

const args = process.argv.slice(2);
const modelIdx = args.indexOf("--model");
const model = modelIdx >= 0 ? args[modelIdx + 1] : undefined;
const stripIdx = args.indexOf("--strip-env");
const stripKeys = stripIdx >= 0 ? args[stripIdx + 1].split(",") : [];
const cwd = args[args.length - 1] ?? process.cwd();

const env = { ...process.env };
for (const key of stripKeys) delete env[key];

const grokArgs = ["--no-auto-update", "agent"];
if (model) grokArgs.push("--model", model);
grokArgs.push("stdio");

console.log(`[SPAWN] grok ${grokArgs.join(" ")}  cwd=${cwd}`);
console.log(`[ENV]   stripped: ${stripKeys.join(",") || "(none)"}`);

const child = spawn("grok", grokArgs, { stdio: ["pipe", "pipe", "pipe"], env, cwd });

let stderrData = "";
child.stderr.on("data", (d) => (stderrData += d.toString()));
child.on("exit", (code) => console.log("\n[EXIT] code=" + code));

const rl = readline.createInterface({ input: child.stdout });
let pending = new Map();
let nextId = 0;

function send(method, params, timeoutMs = 60000) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        reject(new Error(`timeout waiting for ${method}`));
      }
    }, timeoutMs);
  });
}

let lastPromptError = null;
rl.on("line", (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) {
        reject(new Error(`${msg.error.code} ${msg.error.message}`));
      } else {
        resolve(msg.result);
      }
    } else if (msg.method === "notifications/initialized") {
      // ignore
    } else if (msg.method === "session/update" && msg.params?.update?.sessionUpdate === "error") {
      console.log("[SESS-ERROR]", JSON.stringify(msg.params.update).slice(0, 400));
      lastPromptError = msg.params.update;
    } else {
      const p = msg.params?.update?.sessionUpdate ?? msg.method;
      if (!p?.startsWith?.("agent_thought_chunk") && !p?.startsWith?.("agent_message_chunk")) {
        console.log("[NOTIF]", JSON.stringify(msg).slice(0, 250));
      }
    }
  } catch { /* non-JSON or unknown frames: ignore */ }
});

try {
  console.log("== 1. initialize ==");
  const init = await send("initialize", {
    protocolVersion: 1,
    clientCapabilities: { fs: { readTextFile: true, writeTextFile: false }, terminal: false },
  });
  console.log("agentName:", init.agentName, "protocol:", init.protocolVersion);
  console.log("authMethods:", JSON.stringify(init.authMethods ?? "NONE"));

  const has = (w) => (init.authMethods ?? []).some((m) => m?.id === w);
  let methodId;
  if (env.XAI_API_KEY && has("xai.api_key")) methodId = "xai.api_key";
  else if (has("cached_token")) methodId = "cached_token";
  else throw new Error("NO auth method available: " + JSON.stringify(init.authMethods));

  console.log("== 2. authenticate (" + methodId + ") ==");
  const auth = await send("authenticate", { methodId, _meta: { headless: true } });
  console.log("authenticate OK:", JSON.stringify(auth).slice(0, 120));

  console.log("== 3. session/new ==");
  const sess = await send("session/new", { cwd, mcpServers: [] });
  console.log("sessionId:", sess.sessionId);

  console.log("== 4. session/prompt (Reply with exactly: PONG) ==");
  try {
    const promptResult = await send(
      "session/prompt",
      { sessionId: sess.sessionId, prompt: [{ type: "text", text: "Reply with exactly: PONG" }] },
      90000
    );
    console.log("prompt accepted, result:", JSON.stringify(promptResult).slice(0, 200));
    // Wait a bit for streamed completion / error notifications.
    await new Promise((r) => setTimeout(r, 15000));
    console.log("\n=== TURN DONE ===");
  } catch (e) {
    console.log("\n=== PROMPT FAILED: " + e.message);
    console.log("last session error:", JSON.stringify(lastPromptError ?? null).slice(0, 500));
    if (stderrData) console.error("[STDERR]\n" + stderrData.slice(0, 1500));
    process.exitCode = 1;
  }
} catch (e) {
  console.error("\n=== FAILED: " + e.message);
  if (stderrData) console.error("[STDERR]\n" + stderrData.slice(0, 2000));
  process.exitCode = 1;
} finally {
  child.kill();
}
