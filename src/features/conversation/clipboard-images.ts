// GAG-010: Clipboard image extraction — pure Renderer logic, no Tauri imports.
// Covers both "file with a filesystem path" (drag/drop picker) and "blob item
// without any path" (Win+Shift+S screenshots paste as image blobs).

import type { ArtifactBlobInput } from "../../bridge/types";

const IMAGE_MIME_PREFIX = "image/";

/** Maximum bytes accepted per clipboard image (mirrors backend IPC bounds). */
export const MAX_CLIPBOARD_IMAGE_BYTES = 6 * 1024 * 1024;

export interface ClipboardImageFile {
  file: File;
  /** Sanitised display name, e.g. "截图.png". */
  displayName: string;
}

/** Extract image files from a paste/drop DataTransfer (blob items included). */
export function extractImageFiles(
  dataTransfer: DataTransfer | null,
): ClipboardImageFile[] {
  if (!dataTransfer) return [];
  const seen = new Set<string>();
  const result: ClipboardImageFile[] = [];

  const pushFile = (file: File | null | undefined): void => {
    if (!file) return;
    if (!file.type.startsWith(IMAGE_MIME_PREFIX)) return;
    const key = `${file.name}:${file.size}:${file.type}`;
    if (seen.has(key)) return;
    seen.add(key);
    result.push({ file, displayName: displayNameFor(file) });
  };

  // items.getAsFile() is the only path that yields blob images without a
  // filesystem path (Win+Shift+S); files[] covers path-backed drops.
  const items = Array.from(dataTransfer.items ?? []);
  for (const item of items) {
    if (item.kind === "file") pushFile(item.getAsFile());
  }
  for (const file of Array.from(dataTransfer.files ?? [])) {
    pushFile(file);
  }
  return result;
}

function displayNameFor(file: File): string {
  const base = file.name.trim();
  if (base) return sanitizeDisplayName(base);
  const extension = file.type.split("/")[1] ?? "png";
  return `剪贴板图片.${extension}`;
}

/** Strip path separators and control characters from a display name. */
export function sanitizeDisplayName(name: string): string {
  const cleaned = name.replace(/[\\/<>:"|?*\0]/g, "");
  return cleaned.slice(0, 120);
}/** Read a File as base64 without data: URL prefix. */
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("无法读取剪贴板图片"));
    reader.onload = () => {
      const result = String(reader.result ?? "");
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.readAsDataURL(file);
  });
}

/** Convert a clipboard image file into the bridge import payload. */
export async function imageFileToBlobInput(
  image: ClipboardImageFile,
): Promise<ArtifactBlobInput> {
  if (image.file.size > MAX_CLIPBOARD_IMAGE_BYTES) {
    throw new Error("剪贴板图片超过 6 MiB，无法导入");
  }
  return {
    displayName: image.displayName,
    base64Data: await fileToBase64(image.file),
  };
}
