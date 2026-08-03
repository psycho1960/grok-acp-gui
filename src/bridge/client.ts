// GAG-003: DesktopBridge client — the single Renderer seam to the backend.
//
// Uses Tauri's `invoke` and `listen` under the hood.  No other file in
// `src/` is allowed to import `@tauri-apps/api` directly.

import type {
  DesktopBridge,
  DesktopCommand,
  DesktopResult,
  BootstrapSnapshot,
  Unsubscribe,
} from "./types";

function requireTauriHost(): void {
  if (typeof window === "undefined") {
    throw new Error("Grok ACP GUI requires the Windows Tauri host.");
  }
  const host = window as unknown as Record<string, unknown>;
  if (!("__TAURI_INTERNALS__" in host || "__TAURI__" in host)) {
    throw new Error("Grok ACP GUI must run inside the Windows Tauri host.");
  }
}

const EVENT_CHANNEL = "bridge:event";

export function createDesktopBridge(): DesktopBridge {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let invokeFn: ((cmd: string, args?: Record<string, unknown>) => Promise<any>) | null =
    null;
  let listenFn:
    | ((channel: string, cb: (event: { payload: unknown }) => void) => Promise<() => void>)
    | null = null;

  async function ensureTauri(): Promise<void> {
    if (invokeFn && listenFn) return;
    requireTauriHost();
    const tauriCore = await import("@tauri-apps/api/core");
    const tauriEvent = await import("@tauri-apps/api/event");
    invokeFn = tauriCore.invoke;
    listenFn = tauriEvent.listen;
  }

  return {
    async bootstrap(): Promise<BootstrapSnapshot> {
      await ensureTauri();
      return invokeFn!("bootstrap") as Promise<BootstrapSnapshot>;
    },

    async execute(command: DesktopCommand): Promise<DesktopResult> {
      await ensureTauri();
      return invokeFn!("execute", { command }) as Promise<DesktopResult>;
    },

    async subscribe(
      listener: (event: import("./types").TypedDesktopEvent) => void,
    ): Promise<Unsubscribe> {
      await ensureTauri();
      const unlisten = await listenFn!(
        EVENT_CHANNEL,
        (event: { payload: unknown }) => {
          const raw = event.payload;
          if (
            typeof raw === "object" &&
            raw !== null &&
            "type" in raw &&
            typeof (raw as Record<string, unknown>).type === "string" &&
            "timestamp" in raw &&
            "payload" in raw
          ) {
            listener(raw as import("./types").TypedDesktopEvent);
          }
          // Drop malformed events silently — the Renderer must not trust IPC.
        },
      );
      let unsubscribed = false;
      return () => {
        if (!unsubscribed) {
          unsubscribed = true;
          unlisten();
        }
      };
    },
  };
}
