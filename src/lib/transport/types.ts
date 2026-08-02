/** Unsubscribe function returned by transport listeners. */
export type Unsubscribe = () => void;

/** Minimal byte transport contract for ACP JSON-RPC frames. */
export interface AcpTransport {
  send(json: string): Promise<void>;
  onMessage(callback: (json: string) => void): Unsubscribe;
  onClose(callback: (reason?: string) => void): Unsubscribe;
  close(): Promise<void>;
}

export class TransportListeners<T> {
  private readonly callbacks = new Set<(value: T) => void>();

  add(callback: (value: T) => void): Unsubscribe {
    this.callbacks.add(callback);
    return () => this.callbacks.delete(callback);
  }

  emit(value: T): void {
    for (const callback of [...this.callbacks]) callback(value);
  }

  clear(): void {
    this.callbacks.clear();
  }
}
