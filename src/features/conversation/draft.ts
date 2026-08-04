// GAG-008: Per-task composer draft persistence (sessionStorage).

const PREFIX = "gag008:draft:";

export function draftStorageKey(taskId: string): string {
  return `${PREFIX}${taskId}`;
}

export function loadDraft(taskId: string | null | undefined): string {
  if (!taskId || typeof sessionStorage === "undefined") return "";
  try {
    return sessionStorage.getItem(draftStorageKey(taskId)) ?? "";
  } catch {
    return "";
  }
}

export function saveDraft(taskId: string | null | undefined, text: string): void {
  if (!taskId || typeof sessionStorage === "undefined") return;
  try {
    if (!text) {
      sessionStorage.removeItem(draftStorageKey(taskId));
    } else {
      sessionStorage.setItem(draftStorageKey(taskId), text);
    }
  } catch {
    // quota / private mode — ignore
  }
}

export function clearDraft(taskId: string | null | undefined): void {
  saveDraft(taskId, "");
}
