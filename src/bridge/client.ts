// GAG-003: DesktopBridge client — the single Renderer seam to the backend.
//
// Uses Tauri's `invoke` and `listen` under the hood.  No other file in
// `src/` is allowed to import `@tauri-apps/api` directly.

import type {
  DesktopBridge,
  DesktopCommand,
  DesktopEvent,
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
      listener: (event: DesktopEvent) => void,
    ): Promise<Unsubscribe> {
      await ensureTauri();
      const unlisten = await listenFn!(
        EVENT_CHANNEL,
        (event: { payload: unknown }) => {
          listener(event.payload as DesktopEvent);
        },
      );
      return () => {
        unlisten();
      };
    },
  };
}
