export interface BootstrapStatus {
  productName: string;
  version: string;
  platform: string;
  ready: boolean;
}

export interface ProjectPreferences {
  projectPath: string | null;
}

const fallbackVersion = "0.1.16";
const fallbackPreferencesKey = "grok-acp-gui:preferences";

function isTauriHost(): boolean {
  if (typeof window === "undefined") return false;
  const host = window as unknown as Record<string, unknown>;
  return "__TAURI_INTERNALS__" in host || "__TAURI__" in host;
}

export async function bootstrap(): Promise<BootstrapStatus> {
  if (!isTauriHost()) {
    return {
      productName: "Grok ACP GUI",
      version: fallbackVersion,
      platform: "windows",
      ready: true,
    };
  }

  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<BootstrapStatus>("bootstrap");
}

export async function selectProjectDirectory(): Promise<string | null> {
  if (!isTauriHost()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择本地 Git 项目",
  });
  return typeof selected === "string" ? selected : null;
}

export async function loadPreferences(): Promise<ProjectPreferences> {
  if (isTauriHost()) {
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("app-preferences.json");
    return {
      projectPath: (await store.get<string>("projectPath")) ?? null,
    };
  }

  const raw = globalThis.localStorage?.getItem(fallbackPreferencesKey);
  if (!raw) return { projectPath: null };
  try {
    const parsed = JSON.parse(raw) as Partial<ProjectPreferences>;
    return {
      projectPath:
        typeof parsed.projectPath === "string" ? parsed.projectPath : null,
    };
  } catch {
    return { projectPath: null };
  }
}

export async function savePreferences(
  preferences: ProjectPreferences,
): Promise<void> {
  if (isTauriHost()) {
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("app-preferences.json");
    await store.set("projectPath", preferences.projectPath);
    await store.save();
    return;
  }

  globalThis.localStorage?.setItem(
    fallbackPreferencesKey,
    JSON.stringify(preferences),
  );
}
