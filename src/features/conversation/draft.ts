// GAG-008: Durable per-task composer draft persistence.

const PREFIX = "gag008:draft:";

export function draftStorageKey(taskId: string): string {
  return `${PREFIX}${taskId}`;
}

function browserStorage(kind: "localStorage" | "sessionStorage"): Storage | null {
  try {
    if (typeof window === "undefined") return null;
    return window[kind];
  } catch {
    return null;
  }
}

export function loadDraft(taskId: string | null | undefined): string {
  if (!taskId) return "";
  const key = draftStorageKey(taskId);
  try {
    const durable = browserStorage("localStorage")?.getItem(key);
    if (durable != null) return durable;

    // Migrate drafts written by the earlier page-session-only implementation.
    const legacyStorage = browserStorage("sessionStorage");
    const legacy = legacyStorage?.getItem(key);
    if (legacy != null) {
      browserStorage("localStorage")?.setItem(key, legacy);
      legacyStorage?.removeItem(key);
      return legacy;
    }
    return "";
  } catch {
    return "";
  }
}

export function saveDraft(taskId: string | null | undefined, text: string): void {
  if (!taskId) return;
  const key = draftStorageKey(taskId);
  try {
    const durable = browserStorage("localStorage");
    if (!text) {
      durable?.removeItem(key);
    } else {
      durable?.setItem(key, text);
    }
    browserStorage("sessionStorage")?.removeItem(key);
  } catch {
    // quota / private mode — ignore
  }
}

export function clearDraft(taskId: string | null | undefined): void {
  saveDraft(taskId, "");
}
