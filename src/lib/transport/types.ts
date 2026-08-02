/** Unsubscribe function returned by transport listeners. */
export type Unsubscribe = () => void;

/** Minimal ACP JSON-RPC frame transport contract. */
export interface AcpTransport {
  start(): Promise<void>;
  send(json: string): Promise<void>;
  onMessage(callback: (json: string) => void): Unsubscribe;
  onClose(callback: (reason?: string) => void): Unsubscribe;
  close(): Promise<void>;
}
