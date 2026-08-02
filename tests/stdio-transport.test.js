import assert from "node:assert/strict";
import test from "node:test";
import { TextDecoder, TextEncoder } from "node:util";
import { setImmediate } from "node:timers";
import { ReadableStream, WritableStream } from "node:stream/web";

const { StdioTransport } = await import("../src/lib/transport/stdio.ts");

test("StdioTransport frames JSON lines over real Web Streams", async () => {
  let inputController;
  const receivedWrites = [];
  const input = new ReadableStream({
    start(controller) {
      inputController = controller;
    },
  });
  const output = new WritableStream({
    write(chunk) {
      receivedWrites.push(new TextDecoder().decode(chunk));
    },
  });
  const transport = new StdioTransport(input, output);
  const messages = [];
  const closeReasons = [];
  transport.onMessage((message) => messages.push(message));
  transport.onClose((reason) => closeReasons.push(reason));
  const reading = transport.start();

  inputController.enqueue(new TextEncoder().encode('{"jsonrpc":"2.0",'));
  inputController.enqueue(new TextEncoder().encode('"id":1}\n'));
  await transport.send('{"jsonrpc":"2.0","method":"initialize"}');
  await new Promise((resolve) => setImmediate(resolve));
  await transport.close();
  await reading;

  assert.deepEqual(messages, ['{"jsonrpc":"2.0","id":1}']);
  assert.deepEqual(receivedWrites, ['{"jsonrpc":"2.0","method":"initialize"}\n']);
  assert.deepEqual(closeReasons, ["closed by client"]);
});
