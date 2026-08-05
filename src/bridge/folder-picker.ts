// Directory picker helper (Tauri dialog plugin). Not part of DesktopCommand.
// Feature code must not import @tauri-apps/* except through bridge/*.

/**
 * Open a native folder picker when running inside Tauri.
 * Returns a structured result so callers can distinguish a normal cancellation
 * from an unavailable native picker without exposing Tauri internals to UI code.
 */
export interface DirectoryPickerResult {
  path: string | null;
  error?: string;
}

export async function pickDirectory(options?: {
  title?: string;
  defaultPath?: string;
}): Promise<DirectoryPickerResult> {
  try {
    if (typeof window === "undefined") {
      return { path: null, error: "当前环境不支持系统文件夹选择器" };
    }
    const host = window as unknown as Record<string, unknown>;
    if (!("__TAURI_INTERNALS__" in host || "__TAURI__" in host)) {
      return { path: null, error: "浏览器环境请手动粘贴绝对路径" };
    }
    const dialog = await import("@tauri-apps/plugin-dialog");
    const selected = await dialog.open({
      directory: true,
      multiple: false,
      title: options?.title ?? "选择项目文件夹",
      defaultPath: options?.defaultPath,
    });
    if (typeof selected === "string" && selected.length > 0) {
      return { path: selected };
    }
    return { path: null };
  } catch {
    return {
      path: null,
      error: "无法打开系统文件夹选择器，请重试或手动粘贴绝对路径",
    };
  }
}
