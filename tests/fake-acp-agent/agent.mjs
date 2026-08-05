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
//   - "bad-frame":  writes a non-JSON line to stdout
//   - "stderr-flood": writes thousands of lines to stderr
//   - "unknown-method": responds to initialize but sends unknown notifications
//   - "permission": sends a requestPermission after the first prompt
//   - "plan":       sends an updatePlan after the first prompt
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
let authenticated = false;
let activeSessionId = null;

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
  sendResponse(id, { sessionId: activeSessionId });
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
    // Send a requestPermission before responding.
    const permId = `perm-${++requestCounter}`;
    sendNotification('requestPermission', {
      requestId: permId,
      toolCall: {
        toolCallId: `tc-${requestCounter}`,
        title: 'Run bash command',
        kind: 'bash',
      },
      options: [
        { optionId: 'opt-allow-once', name: 'Allow once' },
        { optionId: 'opt-reject', name: 'Reject' },
      ],
    });
  }

  if (SCENARIO === 'plan') {
    // Send an updatePlan.
    const planId = `plan-${++requestCounter}`;
    sendNotification('updatePlan', {
      requestId: planId,
      summary: 'I will create a file and edit another.',
      options: [
        { optionId: 'opt-approve', name: 'Approve' },
        { optionId: 'opt-reject', name: 'Reject' },
      ],
    });
  }

  if (SCENARIO === 'unknown-method') {
    // Send an unknown notification.
    sendNotification('some/future/method', { foo: 'bar' });
  }

  // Stream assistant deltas.
  const words = ['Hello', ' from', ' fake', ' ACP', ' agent!'];
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

function handleResolvePermission(id) {
  sendResponse(id, { resolved: true });
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
      case 'session/cancel':
        handleCancel(msg.id);
        break;
      case 'resolvePermission':
        handleResolvePermission(msg.id);
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
