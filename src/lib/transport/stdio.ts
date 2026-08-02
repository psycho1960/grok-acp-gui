import type { AcpTransport, Unsubscribe } from "./types";

/**
 * Production stdio transport over the streams owned by the desktop host.
 *
 * GAG-001 does not create a subprocess. The later ACP runtime supplies the
 * actual process streams; this class owns framing, delivery, and cleanup.
 */
export class StdioTransport implements AcpTransport {
  private readonly decoder = new TextDecoder();
  private readonly encoder = new TextEncoder();
  private readonly reader: ReadableStreamDefaultReader<Uint8Array>;
  private readonly writer: WritableStreamDefaultWriter<Uint8Array>;
  private readonly messageListeners = new Set<(json: string) => void>();
  private readonly closeListeners = new Set<(reason?: string) => void>();
  private readTask: Promise<void> | null = null;
  private closeTask: Promise<void> | null = null;
  private closed = false;

  constructor(
    input: ReadableStream<Uint8Array>,
    output: WritableStream<Uint8Array>,
  ) {
    this.reader = input.getReader();
    this.writer = output.getWriter();
  }

  start(): Promise<void> {
    if (!this.readTask) this.readTask = this.consumeInput();
    return this.readTask;
  }

  async send(json: string): Promise<void> {
    if (this.closed) throw new Error("StdioTransport is closed");
    await this.writer.write(this.encoder.encode(`${json}\n`));
  }

  onMessage(callback: (json: string) => void): Unsubscribe {
    this.messageListeners.add(callback);
    return () => this.messageListeners.delete(callback);
  }

  onClose(callback: (reason?: string) => void): Unsubscribe {
    this.closeListeners.add(callback);
    return () => this.closeListeners.delete(callback);
  }

  close(): Promise<void> {
    return this.finish("closed by client");
  }

  private async consumeInput(): Promise<void> {
    let buffer = "";
    try {
      while (!this.closed) {
        const { done, value } = await this.reader.read();
        if (done) break;
        buffer += this.decoder.decode(value, { stream: true });
        buffer = this.emitFrames(buffer);
      }
      buffer += this.decoder.decode();
      if (buffer.trim()) this.emitFrame(buffer.trim());
    } catch (error) {
      if (!this.closed) {
        await this.finish(
          error instanceof Error ? error.message : String(error),
        );
      }
      return;
    }

    if (!this.closed) {
      await this.finish("stdio stream ended");
    }
  }

  private finish(reason?: string): Promise<void> {
    if (!this.closeTask) {
      this.closed = true;
      this.closeTask = (async () => {
        await Promise.allSettled([this.reader.cancel(), this.writer.close()]);
        this.reader.releaseLock();
        this.writer.releaseLock();
        this.emitClose(reason);
      })();
    }
    return this.closeTask;
  }

  private emitFrames(buffer: string): string {
    let newlineIndex = buffer.indexOf("\n");
    while (newlineIndex >= 0) {
      const frame = buffer.slice(0, newlineIndex).trim();
      if (frame) this.emitFrame(frame);
      buffer = buffer.slice(newlineIndex + 1);
      newlineIndex = buffer.indexOf("\n");
    }
    return buffer;
  }

  private emitFrame(frame: string): void {
    for (const callback of [...this.messageListeners]) callback(frame);
  }

  private emitClose(reason?: string): void {
    for (const callback of [...this.closeListeners]) callback(reason);
    this.messageListeners.clear();
    this.closeListeners.clear();
  }
}
