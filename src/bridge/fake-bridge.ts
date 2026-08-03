// GAG-003: FakeDesktopBridge for testing and UI development.
//
// An in-memory implementation that all Feature tests and the UiKitFixture
// can use.  No Tauri host required.

import type {
  DesktopBridge,
  DesktopCommand,
  DesktopResult,
  BootstrapSnapshot,
  Unsubscribe,
  AppError,
  TypedDesktopEvent,
} from "./types";

type Listener = (event: TypedDesktopEvent) => void;

export interface FakeBridgeOptions {
  /** Initial bootstrap snapshot returned by `bootstrap()`. */
  bootstrapSnapshot?: Partial<BootstrapSnapshot>;
  /**
   * Handler for `execute()`.  Receives every command; return a result.
   * Default: returns `{ acknowledged: command.type }` for every command.
   */
  onExecute?: (command: DesktopCommand) => DesktopResult | Promise<DesktopResult>;
}

export function createFakeDesktopBridge(
  options: FakeBridgeOptions = {},
): DesktopBridge & {
  /** Push an event to all registered listeners (for test orchestration). */
  pushEvent(event: TypedDesktopEvent): void;
} {
  const listeners = new Set<Listener>();

  const defaultSnapshot: BootstrapSnapshot = {
    productName: "Grok ACP GUI (fake)",
    version: "0.0.0-test",
    platform: "win32",
    ready: true,
    runtime: { status: "ready" },
    capabilities: { models: [], modes: [], slashCommands: [] },
    ...options.bootstrapSnapshot,
  };

  return {
    async bootstrap(): Promise<BootstrapSnapshot> {
      return { ...defaultSnapshot };
    },

    async execute(command: DesktopCommand): Promise<DesktopResult> {
      if (options.onExecute) {
        return options.onExecute(command);
      }
      return {
        success: "true",
        data: { acknowledged: command.type },
      };
    },

    async subscribe(listener: Listener): Promise<Unsubscribe> {
      listeners.add(listener);
      let unsubscribed = false;
      return () => {
        if (!unsubscribed) {
          listeners.delete(listener);
          unsubscribed = true;
        }
      };
    },

    pushEvent(event: TypedDesktopEvent): void {
      for (const listener of listeners) {
        listener(event);
      }
    },
  };
}

/** Create an AppError stub for test assertions. */
export function fakeError(
  overrides: Partial<AppError> = {},
): AppError {
  return {
    code: "TEST",
    message: "test error",
    retryable: false,
    detailsRedacted: true,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    correlationId: "00000000test0000" as any,
    ...overrides,
  };
}
