// Fake ACP Agent — simulates the Grok CLI `agent stdio` mode for
// integration tests.  Reads JSON-RPC 2.0 from stdin, writes JSON-RPC
// 2.0 to stdout, and logs to stderr.
//
// Scenario selection is controlled by the FAKE_ACP_SCENARIO env var:
//   - "normal"      (default): full happy-path handshake + streaming
//   - "slow":       same lifecycle with a deliberately slow turn
//   - "timeout":    accepts initialize but never responds (handshake timeout)
//   - "crash":      exits immediately with code 1 after receiving initialize
//   - "crash-after-prompt": exits after accepting the first prompt
//   - "turn-auth-required": handshake succeeds, first Turn returns a safe 401
//   - "bad-frame":  writes a non-JSON line to stdout
//   - "stderr-flood": writes thousands of lines to stderr
//   - "set-mode-error": rejects every session/set_mode request
//   - "unknown-method": responds to initialize but sends unknown notifications
//   - "permission": sends a requestPermission after the first prompt
//   - "plan":       sends an updatePlan after the first prompt
//   - "plan-permission": sends updatePlan first, then a standard ACP v1
//                        requestPermission whose toolCall.rawInput carries
//                        a plain command (no private "operation" field)
//   - "process-write":  sends a standard ACP v1 requestPermission whose
//                        toolCall.rawInput.command is a non-git Process
//                        (e.g. "npm install pkg") — no private "operation",
//                        no platform PathOptions. The single-purpose
//                        permission request lets the agent notice the
//                        request immediately so the test can verify
//                        adapter→TaskRuntime fail-closed for Process.
//   - "process-escape": sends a destructive command that explicitly
//                        targets a workspace-external path. Reproduces
//                        the P1-02 cwd-escape scenario: rawInput.command
//                        carries an out-of-workspace target, no cwd, no
//                        write_paths. Adapter must fail-closed regardless.
//   - "read-escape": sends an apparently read-only rawInput command whose
//                        literal path operand is outside the workspace.
//   - "secret-display-fields": sends a permission request containing a
//                        fixture secret in every Renderer-visible display
//                        field. The client must redact those fields before
//                        persistence and bridge publication.
//   - "duplicate-permission": emits two permission requests with the same
//                        business requestId and distinct JSON-RPC ids.
//   - "duplicate-plan": emits two Plan requests with the same business
//                        requestId and distinct JSON-RPC ids.
//
// Usage:
//   FAKE_ACP_SCENARIO=normal node agent.mjs
//
// Security: this script never reads or writes real credentials.

import * as readline from 'node:readline';

const SCENARIO = process.env.FAKE_ACP_SCENARIO || 'normal';
const PROTOCOL_VERSION = 1;

// --- JSON-RPC helpers ---

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}

function sendResponse(id, result) {
  send({ jsonrpc: '2.0', id, result });
}

function sendError(id, code, message) {
  send({ jsonrpc: '2.0', id, error: { code, message, data: null } });
}

function sendNotification(method, params) {
  send({ jsonrpc: '2.0', method, params });
}

function logErr(msg) {
  process.stderr.write(`[fake-acp] ${msg}\n`);
}

// --- Scenario handlers ---

let requestCounter = 0;
let serverRequestCounter = 1000;
let authenticated = false;
let activeSessionId = null;
let pendingPermissionResponseId = null;
let pendingPlanResponseId = null;
// Decision gates: the turn resumes only after every agent-to-client
// request (permission/plan) the scenario emitted has been answered.
const pendingGates = [];
const resolvedGates = new Set();
let pendingPrompt = null;
let activeMode = 'default';

function allGatesResolved() {
  return pendingGates.length > 0 && pendingGates.every((gate) => resolvedGates.has(gate));
}

function handleInitialize(id) {
  if (SCENARIO === 'timeout') {
    // Accept the request but never respond.
    logErr('scenario=timeout: deliberately not responding to initialize');
    return;
  }

  if (SCENARIO === 'crash') {
    logErr('scenario=crash: crashing after initialize');
    process.exit(1);
  }

  if (SCENARIO === 'bad-frame') {
    // Write a non-JSON line to stdout.
    process.stdout.write('this is not valid JSON\n');
    // Then respond normally.
  }

  sendResponse(id, {
    protocolVersion: PROTOCOL_VERSION,
    agentName: 'fake-grok',
    agentVersion: '0.2.118',
    agentCapabilities: {
      fs: true,
      terminal: true,
    },
    instructions: 'Fake ACP agent for testing.',
    authMethods: [{ id: 'cached_token', name: 'Cached login' }],
    models: [
      { modelId: 'grok-4', name: 'Grok 4' },
    ],
    modes: [
      { id: 'code', name: 'Code' },
      { id: 'ask', name: 'Ask' },
    ],
  });
}

function handleAuthenticate(id, params) {
  if (params?.methodId !== 'cached_token') {
    sendError(id, -32602, 'unsupported auth method');
    return;
  }
  authenticated = true;
  sendResponse(id, {});
}

function handleSessionNew(id, params) {
  if (!authenticated) {
    sendError(id, -32000, 'authentication required');
    return;
  }
  if (typeof params?.cwd !== 'string' || !Array.isArray(params?.mcpServers)) {
    sendError(id, -32602, 'cwd and mcpServers are required');
    return;
  }
  activeSessionId = `fake-session-${++requestCounter}`;
  // Grok Build may advertise commands before acknowledging session/new.
  // Keep this scenario interleaved so the Runtime must preserve handshake
  // notifications until its normal reader loop is running.
  if (SCENARIO === 'available-commands') {
    sendNotification('session/update', {
      sessionId: activeSessionId,
      update: {
        sessionUpdate: 'available_commands_update',
        availableCommands: [
          { name: 'init', description: 'Initialize a new project', input: null },
          { name: 'plan', description: 'Plan a change', input: { unstructured: true } },
        ],
      },
    });
  }
  sendResponse(id, {
    sessionId: activeSessionId,
    modes: {
      currentModeId: activeMode,
      availableModes: [
        { id: 'default', name: 'Default' },
        { id: 'plan', name: 'Plan' },
        { id: 'code', name: 'Code' },
        { id: 'ask', name: 'Ask' },
      ],
    },
  });
}

function handleSetMode(id, params) {
  const allowed = new Set(['default', 'plan', 'code', 'ask']);
  if (params?.sessionId !== activeSessionId || !allowed.has(params?.modeId)) {
    sendError(id, -32602, 'sessionId and an advertised modeId are required');
    return;
  }
  if (SCENARIO === 'set-mode-error') {
    sendError(id, -32000, 'mode change rejected by fake agent');
    return;
  }
  activeMode = params.modeId;
  sendResponse(id, {});
}

function handlePrompt(id, params) {
  if (
    params?.sessionId !== activeSessionId ||
    !Array.isArray(params?.prompt) ||
    params.prompt[0]?.type !== 'text' ||
    typeof params.prompt[0]?.text !== 'string'
  ) {
    sendError(id, -32602, 'sessionId and text prompt content blocks are required');
    return;
  }

  if (SCENARIO === 'turn-auth-required') {
    sendError(id, -32001, '401 Unauthorized: authentication required');
    return;
  }

  sendNotification('session/update', {
    sessionId: activeSessionId,
    update: {
      sessionUpdate: 'user_message_chunk',
      content: { type: 'text', text: params.prompt[0].text },
    },
  });
  if (SCENARIO === 'crash-after-prompt') {
    setTimeout(() => process.exit(2), 10);
    return;
  }
  if (SCENARIO === 'permission') {
    // ACP v1: permission is an agent-to-client JSON-RPC request. The client
    // must answer this exact id with result.outcome; there is no
    // resolvePermission method.
    const permId = `perm-${++requestCounter}`;
    pendingPermissionResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPermissionResponseId, method: 'session/request_permission', params: {
      requestId: permId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Run bash command',
        kind: 'bash',
        rawInput: { command: 'git commit -m test' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    // The agent cannot continue the turn until the client responds.
    pendingGates.push('permission');
  }

  if (SCENARIO === 'duplicate-permission') {
    const requestId = 'duplicate-perm-1';
    const firstRpcId = ++serverRequestCounter;
    pendingPermissionResponseId = firstRpcId;
    send({ jsonrpc: '2.0', id: firstRpcId, method: 'session/request_permission', params: {
      requestId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: 'tc-duplicate-first',
        title: 'First request',
        kind: 'bash',
        rawInput: { command: 'git commit -m first' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    const secondRpcId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: secondRpcId, method: 'session/request_permission', params: {
      requestId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: 'tc-duplicate-second',
        title: 'Second request must be rejected',
        kind: 'bash',
        rawInput: { command: 'rm.exe D:/outside/victim.txt' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    pendingGates.push('permission');
  }

  if (SCENARIO === 'duplicate-plan') {
    const requestId = 'duplicate-plan-1';
    const firstRpcId = ++serverRequestCounter;
    pendingPlanResponseId = firstRpcId;
    send({ jsonrpc: '2.0', id: firstRpcId, method: 'updatePlan', params: {
      requestId,
      summary: 'First Plan request',
      options: [
        { optionId: 'opt-approve', name: 'Approve', kind: 'approve' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject' },
      ],
    }});
    const secondRpcId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: secondRpcId, method: 'updatePlan', params: {
      requestId,
      summary: 'Second Plan request must be rejected',
      options: [
        { optionId: 'opt-approve', name: 'Approve', kind: 'approve' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject' },
      ],
    }});
    pendingGates.push('plan');
  }

  if (SCENARIO === 'process-escape') {
    // Destructive command targeting an explicit workspace-external path.
    // Matches the P1-02 cwd-escape failure mode: rawInput.command points
    // outside the workspace, no cwd/write_paths supplied. The adapter
    // must still fail-closed (no let-through allow_once).
    const permId = `perm-${++requestCounter}`;
    pendingPermissionResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPermissionResponseId, method: 'session/request_permission', params: {
      requestId: permId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Remove file outside workspace',
        kind: 'bash',
        rawInput: { command: 'rm.exe D:/outside/victim.txt' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    pendingGates.push('permission');
  }

  if (SCENARIO === 'secret-display-fields') {
    const permId = `perm-${++requestCounter}`;
    pendingPermissionResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPermissionResponseId, method: 'session/request_permission', params: {
      requestId: permId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Deploy token=GAG009_TEST_SECRET_NEVER_LOG',
        kind: 'bash',
        locations: [{ path: 'C:/repo/token=GAG009_TEST_SECRET_NEVER_LOG' }],
        rawInput: { command: 'git commit -m test' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow token=GAG009_TEST_SECRET_NEVER_LOG', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    pendingGates.push('permission');
  }

  if (SCENARIO === 'read-escape') {
    const permId = `perm-${++requestCounter}`;
    pendingPermissionResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPermissionResponseId, method: 'session/request_permission', params: {
      requestId: permId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Search outside workspace',
        kind: 'bash',
        rawInput: { command: 'rg secret D:/outside' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    pendingGates.push('permission');
  }

  if (SCENARIO === 'process-write') {
    // Standard ACP v1 process write request: rawInput.command is a non-git
    // executable (npm install). No private "operation" field, no PathOptions.
    // The adapter must classify this as OperationCategory::Process, then
    // ExecutionGuard must fail-closed (no cwd → validate_within rejects →
    // allow actions are stripped). The agent only emits the request and
    // waits for the resolution before continuing.
    const permId = `perm-${++requestCounter}`;
    pendingPermissionResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPermissionResponseId, method: 'session/request_permission', params: {
      requestId: permId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Install npm package',
        kind: 'bash',
        rawInput: { command: 'npm install vitest' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    pendingGates.push('permission');
  }

  if (SCENARIO === 'plan-permission') {
    // Plan decision first: same agent-to-client request shape as the
    // permission decision below. Named kinds must stay explicit so the
    // production interpreter can map approve/reject/revise without
    // guessing semantics from labels.
    const planId = `plan-${++requestCounter}`;
    pendingPlanResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPlanResponseId, method: 'updatePlan', params: {
      requestId: planId,
      summary: 'I will commit a fixture change and edit another file.',
      options: [
        { optionId: 'opt-approve', name: 'Approve', kind: 'approve' },
        { optionId: 'opt-revise', name: 'Request changes', kind: 'revision_requested' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject' },
      ],
    }});
    // Then a standard ACP v1 write request: rawInput.command only, with no
    // project-private "operation" field. The client must classify it as a
    // git write and preserve the original allow option id.
    const permId = `perm-${++requestCounter}`;
    pendingPermissionResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPermissionResponseId, method: 'session/request_permission', params: {
      requestId: permId,
      sessionId: activeSessionId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Run git commit',
        kind: 'bash',
        rawInput: { command: 'git commit -m test' },
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once', kind: 'allow_once' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject_once' },
      ],
    }});
    // The turn resumes only after BOTH decisions are answered.
    pendingGates.push('plan', 'permission');
  }

  if (SCENARIO === 'plan') {
    // Plan decisions are agent-to-client JSON-RPC responses, just like
    // permission decisions.  A notification has no response id and cannot
    // implement approve/revise/reject.
    const planId = `plan-${++requestCounter}`;
    pendingPlanResponseId = ++serverRequestCounter;
    send({ jsonrpc: '2.0', id: pendingPlanResponseId, method: 'updatePlan', params: {
      requestId: planId,
      summary: 'I will create a file and edit another.',
      options: [
        { optionId: 'opt-approve', name: 'Approve', kind: 'approve' },
        { optionId: 'opt-reject', name: 'Reject', kind: 'reject' },
      ],
    }});
    pendingGates.push('plan');
  }

  if (SCENARIO === 'unknown-method') {
    // Send an unknown notification.
    sendNotification('some/future/method', { foo: 'bar' });
  }

  if (pendingGates.length === 0) {
    streamAndFinish(id, params);
  } else {
    // Remember the prompt so the turn can finish once all gates resolve.
    pendingPrompt = { id, params };
  }
}

// Stream the assistant delta, a tool call, and the end_turn response.
function streamAndFinish(id, params) {
  const hasImage = params.prompt.some((block) => block?.type === 'image');
  const text = params.prompt.find((block) => block?.type === 'text')?.text || '';
  const words = hasImage
    ? ['VISUAL_CONTEXT_OK']
    : text.includes('<attachment_visual_context')
      ? ['MAIN_TEXT_ONLY_OK']
      : ['Hello', ' from', ' fake', ' ACP', ' agent!'];
  // Echo the active mode so tests can observe session/set_mode, plus the
  // per-turn model/reasoning params when present.
  if (params.model !== undefined || params.reasoning !== undefined) {
    words.unshift(`MODEL=${params.model ?? '-'} REASONING=${params.reasoning ?? '-'} MODE=${activeMode}`);
  } else {
    words.unshift(`MODE=${activeMode}`);
  }
  let delay = SCENARIO === 'slow' ? 200 : 10;
  const step = SCENARIO === 'slow' ? 100 : 10;
  for (const word of words) {
    setTimeout(() => {
      sendNotification('session/update', {
        sessionId: activeSessionId,
        update: {
          sessionUpdate: 'agent_message_chunk',
          content: { type: 'text', text: word },
        },
      });
    }, delay);
    delay += step;
  }

  // Send a tool call.
  setTimeout(() => {
    sendNotification('session/update', {
      sessionId: activeSessionId,
      update: {
        sessionUpdate: 'tool_call',
        toolCallId: `tc-${++requestCounter}`,
        title: 'Edit file',
        kind: 'edit',
        status: 'in_progress',
        rawInput: { path: 'fixture.txt' },
      },
    });
  }, delay);
  delay += step;

  // Tool complete.
  setTimeout(() => {
    sendNotification('session/update', {
      sessionId: activeSessionId,
      update: {
        sessionUpdate: 'tool_call_update',
        toolCallId: `tc-${requestCounter}`,
        status: 'completed',
        rawOutput: { linesChanged: 1 },
      },
    });
  }, delay);
  delay += step;

  // Respond to the prompt request.
  setTimeout(() => {
    sendResponse(id, { stopReason: 'end_turn' });
  }, delay);
}

function handleCancel(id) {
  sendResponse(id, { cancelled: true });
}

function handleResolvePlan(id) {
  sendResponse(id, { resolved: true });
}

// --- stderr flood ---

if (SCENARIO === 'stderr-flood') {
  let count = 0;
  const interval = setInterval(() => {
    logErr(`flood line ${count++} token=fake_secret_value`);
    if (count > 5000) {
      clearInterval(interval);
    }
  }, 1);
}

// --- Main loop ---

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false,
});

logErr(`fake-acp-agent started (scenario=${SCENARIO}, pid=${process.pid})`);

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  let msg;
  try {
    msg = JSON.parse(trimmed);
  } catch (e) {
    logErr(`invalid JSON on stdin: ${e.message}`);
    return;
  }

  // Handle responses to agent-to-client requests first.
  if (msg.id !== undefined && !msg.method) {
    if (msg.id === pendingPermissionResponseId) {
      const selected = msg.result?.outcome;
      if (selected?.outcome !== 'selected' || typeof selected.optionId !== 'string') {
        logErr('invalid permission response outcome');
        process.exitCode = 3;
      }
      pendingPermissionResponseId = null;
      resolvedGates.add('permission');
    }
    if (msg.id === pendingPlanResponseId) {
      const selected = msg.result?.outcome;
      if (selected?.outcome !== 'selected' || typeof selected.optionId !== 'string') {
        logErr('invalid plan response outcome');
        process.exitCode = 3;
      }
      pendingPlanResponseId = null;
      resolvedGates.add('plan');
    }
    if (allGatesResolved() && pendingPrompt) {
      const { id: promptId, params: promptParams } = pendingPrompt;
      pendingPrompt = null;
      streamAndFinish(promptId, promptParams);
    }
    return;
  }

  // Handle as request (has id) or notification (no id).
  if (msg.id !== undefined && msg.method) {
    switch (msg.method) {
      case 'initialize':
        handleInitialize(msg.id);
        break;
      case 'authenticate':
        handleAuthenticate(msg.id, msg.params);
        break;
      case 'session/new':
        handleSessionNew(msg.id, msg.params);
        break;
      case 'session/prompt':
        handlePrompt(msg.id, msg.params);
        break;
      case 'session/set_mode':
        handleSetMode(msg.id, msg.params);
        break;
      case 'session/cancel':
        handleCancel(msg.id);
        break;
      case 'resolvePlan':
        handleResolvePlan(msg.id);
        break;
      default:
        sendError(msg.id, -32601, `method not found: ${msg.method}`);
    }
  } else if (msg.method) {
    // Notification — no response.
    logErr(`notification: ${msg.method}`);
  }
});

rl.on('close', () => {
  logErr('stdin closed, exiting');
  process.exit(0);
});

// Handle SIGTERM / SIGINT gracefully.
process.on('SIGTERM', () => {
  logErr('received SIGTERM, exiting');
  process.exit(0);
});

process.on('SIGINT', () => {
  logErr('received SIGINT, exiting');
  process.exit(0);
});
