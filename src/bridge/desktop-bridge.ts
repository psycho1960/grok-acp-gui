// GAG-003: This file is retained for backward-compatibility with GAG-001/002
// consumers that import `bootstrap` directly.  New code should use
// `createDesktopBridge()` from `src/bridge/client.ts`.

import { createDesktopBridge } from "./client.ts";
export { type BootstrapStatus } from "./types.ts";

/**
 * Backward-compatible one-shot bootstrap that returns the raw
 * BootstrapStatus.  Feature code should prefer `createDesktopBridge()`
 * and call `.bootstrap()` on the returned bridge.
 */
export async function bootstrap(): Promise<import("./types").BootstrapStatus> {
  const bridge = createDesktopBridge();
  return bridge.bootstrap();
}
