import { reactive, readonly } from "vue";

export type ToastTone = "success" | "warning" | "error" | "info";

export type ToastItem = {
  id: string;
  tone: ToastTone;
  title: string;
  description?: string;
  /** ms; 0 = sticky until dismissed */
  duration: number;
  createdAt: number;
};

export type ToastOptions = {
  description?: string;
  duration?: number;
};

const MAX_TOASTS = 3;

const state = reactive<{ items: ToastItem[] }>({ items: [] });
const timers = new Map<string, ReturnType<typeof setTimeout>>();

let seq = 0;

function defaultDuration(tone: ToastTone): number {
  if (tone === "error") return 0;
  if (tone === "warning") return 5000;
  return 3000;
}

function remove(id: string): void {
  const timer = timers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    timers.delete(id);
  }
  const index = state.items.findIndex((item) => item.id === id);
  if (index >= 0) state.items.splice(index, 1);
}

function push(tone: ToastTone, title: string, options: ToastOptions = {}): string {
  const id = `toast-${++seq}-${Date.now()}`;
  const duration = options.duration ?? defaultDuration(tone);
  const item: ToastItem = {
    id,
    tone,
    title,
    description: options.description,
    duration,
    createdAt: Date.now(),
  };
  state.items.unshift(item);
  while (state.items.length > MAX_TOASTS) {
    const dropped = state.items.pop();
    if (dropped) remove(dropped.id);
  }
  if (duration > 0) {
    timers.set(
      id,
      setTimeout(() => {
        remove(id);
      }, duration),
    );
  }
  return id;
}

export const toast = {
  items: readonly(state).items,
  success(title: string, options?: ToastOptions): string {
    return push("success", title, options);
  },
  warning(title: string, options?: ToastOptions): string {
    return push("warning", title, options);
  },
  error(title: string, options?: ToastOptions): string {
    return push("error", title, options);
  },
  info(title: string, options?: ToastOptions): string {
    return push("info", title, options);
  },
  dismiss(id: string): void {
    remove(id);
  },
  clear(): void {
    for (const item of [...state.items]) remove(item.id);
  },
};

/** Mutable list for the host (readonly proxy items is for consumers). */
export function useToastState(): { items: ToastItem[] } {
  return state;
}
