import { createFakeDesktopBridge } from "../../src/bridge/fake-bridge";
import type { DesktopBridge, TypedDesktopEvent } from "../../src/bridge/types";

export interface FakeAcpScript {
  readonly scenario:
    | "normal"
    | "slow"
    | "bad-frame"
    | "permission"
    | "plan"
    | "crash"
    | "stderr-flood";
  readonly scriptPath: string;
  readonly isolated: true;
}

export interface TempRepositoryFixture {
  readonly root: string;
  readonly commonGitDir: string;
  readonly isolated: true;
}

export interface TempSqliteFixture {
  readonly databasePath: string;
  readonly isolated: true;
}

export interface FaultInjector {
  inject(point: string, action: () => void): void;
  visited(): readonly string[];
}

export interface EvidenceEntry {
  name: string;
  value: number | string | boolean;
  unit?: string;
}

export class EvidenceRecorder {
  readonly entries: EvidenceEntry[] = [];

  record(entry: EvidenceEntry): void {
    this.entries.push({ ...entry });
  }
}

/** Renderer-side scenario over the production DesktopBridge interface. */
export class FakeDesktopBridgeScenario {
  readonly bridge: DesktopBridge & {
    pushEvent(event: TypedDesktopEvent): void;
  };

  constructor(
    readonly name: string,
    onExecute?: Parameters<typeof createFakeDesktopBridge>[0],
  ) {
    this.bridge = createFakeDesktopBridge(onExecute);
  }

  emit(event: TypedDesktopEvent): void {
    this.bridge.pushEvent(event);
  }
}
