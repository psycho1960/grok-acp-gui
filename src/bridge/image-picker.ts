// Image picker helper (Tauri dialog plugin). Feature code never imports
// @tauri-apps APIs directly.

export interface ImagePickerResult {
  paths: string[];
  error?: string;
}

export async function pickImages(): Promise<ImagePickerResult> {
  try {
    if (typeof window === "undefined") {
      return { paths: [], error: "当前环境不支持系统图片选择器" };
    }
    const host = window as unknown as Record<string, unknown>;
    if (!("__TAURI_INTERNALS__" in host || "__TAURI__" in host)) {
      return { paths: [], error: "图片选择仅在桌面应用中可用" };
    }
    const dialog = await import("@tauri-apps/plugin-dialog");
    const selected = await dialog.open({
      directory: false,
      multiple: true,
      title: "添加图片附件",
      filters: [
        {
          name: "图片",
          extensions: ["png", "jpg", "jpeg", "gif", "webp"],
        },
      ],
    });
    if (Array.isArray(selected)) {
      return { paths: selected.filter((path): path is string => typeof path === "string") };
    }
    return { paths: typeof selected === "string" ? [selected] : [] };
  } catch {
    return { paths: [], error: "无法打开系统图片选择器，请重试" };
  }
}
