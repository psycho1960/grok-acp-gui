import type { Unsubscribe } from "./types";

export type ImageDropEvent =
  | { type: "hover"; clientX: number; clientY: number }
  | { type: "drop"; clientX: number; clientY: number; paths: string[] }
  | { type: "leave" };

/**
 * Subscribe to native desktop file drops without exposing Tauri to feature code.
 * Tauri reports physical pixels while DOM hit testing uses CSS pixels.
 */
export async function subscribeImageDrops(
  listener: (event: ImageDropEvent) => void,
): Promise<Unsubscribe> {
  if (typeof window === "undefined") return () => undefined;
  const host = window as unknown as Record<string, unknown>;
  if (!("__TAURI_INTERNALS__" in host || "__TAURI__" in host)) {
    return () => undefined;
  }

  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (payload.type === "leave") {
      listener({ type: "leave" });
      return;
    }

    const scale = window.devicePixelRatio || 1;
    const point = {
      clientX: payload.position.x / scale,
      clientY: payload.position.y / scale,
    };
    if (payload.type === "drop") {
      listener({ type: "drop", ...point, paths: payload.paths });
    } else {
      listener({ type: "hover", ...point });
    }
  });
}
