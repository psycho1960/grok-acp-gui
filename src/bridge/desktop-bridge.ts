export interface BootstrapStatus {
  productName: string;
  version: string;
  platform: string;
  ready: boolean;
}

function requireTauriHost(): void {
  if (typeof window === "undefined") {
    throw new Error("Grok ACP GUI requires the Windows Tauri host.");
  }
  const host = window as unknown as Record<string, unknown>;
  if (!("__TAURI_INTERNALS__" in host || "__TAURI__" in host)) {
    throw new Error("Grok ACP GUI must run inside the Windows Tauri host.");
  }
}

export async function bootstrap(): Promise<BootstrapStatus> {
  requireTauriHost();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<BootstrapStatus>("bootstrap");
}

export async function selectProjectDirectory(): Promise<string | null> {
  requireTauriHost();
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择本地 Git 项目",
  });
  return typeof selected === "string" ? selected : null;
}
