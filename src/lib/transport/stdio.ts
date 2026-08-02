import { TransportListeners, type AcpTransport, type Unsubscribe } from "./types";

/**
 * The desktop ACP stdio seam retained from the upstream baseline.
 *
 * Process creation and JSON-RPC session orchestration are intentionally not
 * part of GAG-001. GAG-005 will provide the guarded Grok ACP adapter. This
 * class keeps the byte transport contract small and testable without exposing
 * arbitrary subprocess controls to the renderer.
 */
export interface StdioTransportPort {
  send(json: string): Promise<void>;
  onMessage(callback: (json: string) => void): Unsubscribe;
  onClose(callback: (reason?: string) => void): Unsubscribe;
  close(): Promise<void>;
}

export class StdioTransport implements AcpTransport {
  private readonly messages = new TransportListeners<string>();
  private readonly closes = new TransportListeners<string | undefined>();
  private readonly unlistenMessage: Unsubscribe;
  private readonly unlistenClose: Unsubscribe;
  private closed = false;

  constructor(private readonly port: StdioTransportPort) {
    this.unlistenMessage = port.onMessage((json) => this.messages.emit(json));
    this.unlistenClose = port.onClose((reason) => {
      this.closed = true;
      this.closes.emit(reason);
    });
  }

  async send(json: string): Promise<void> {
    if (this.closed) throw new Error("StdioTransport is closed");
    await this.port.send(json);
  }

  onMessage(callback: (json: string) => void): Unsubscribe {
    return this.messages.add(callback);
  }

  onClose(callback: (reason?: string) => void): Unsubscribe {
    return this.closes.add(callback);
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.unlistenMessage();
    this.unlistenClose();
    await this.port.close();
    this.messages.clear();
    this.closes.clear();
  }
}
