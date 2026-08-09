// Native save-dialog boundary for Artifact exports. Feature code never imports
// Tauri APIs directly and receives only the path explicitly chosen by the user.

import type { ArtifactDescriptor, ArtifactSaveResult } from "./types";

export type ArtifactSavePickerResult =
  | { status: "selected"; path: string }
  | (Pick<ArtifactSaveResult, "artifactId" | "message"> & { status: "cancelled" })
  | (Pick<ArtifactSaveResult, "artifactId" | "message"> & { status: "failed" });

const extensionsByMime: Record<string, string[]> = {
  "image/png": ["png"],
  "image/jpeg": ["jpg", "jpeg", "jpe"],
  "image/webp": ["webp"],
  "image/gif": ["gif"],
  "image/bmp": ["bmp", "dib"],
  "image/x-icon": ["ico", "cur"],
  "image/tiff": ["tif", "tiff"],
  "image/avif": ["avif"],
};

export async function pickArtifactSavePath(
  artifact: ArtifactDescriptor,
): Promise<ArtifactSavePickerResult> {
  try {
    if (typeof window === "undefined") {
      return { status: "failed", artifactId: artifact.artifactId, message: "当前环境不支持系统保存对话框" };
    }
    const host = window as unknown as Record<string, unknown>;
    if (!("__TAURI_INTERNALS__" in host || "__TAURI__" in host)) {
      return { status: "failed", artifactId: artifact.artifactId, message: "当前环境不支持系统保存对话框" };
    }
    const dialog = await import("@tauri-apps/plugin-dialog");
    const extensions = extensionsByMime[artifact.mimeType];
    const selected = await dialog.save({
      title: `另存为 ${artifact.displayName}`,
      defaultPath: artifact.displayName,
      filters: extensions ? [{ name: artifact.mimeType, extensions }] : undefined,
    });
    return typeof selected === "string" && selected.length > 0
      ? { status: "selected", path: selected }
      : {
          status: "cancelled",
          artifactId: artifact.artifactId,
          message: "已取消另存为，未修改任何文件",
        };
  } catch {
    return {
      status: "failed",
      artifactId: artifact.artifactId,
      message: "无法打开系统保存对话框，请重试",
    };
  }
}
