// GAG-008: Hash routes for conversation + deep-link to event seq.

const PREFIX = "#conversation";

export interface ConversationRoute {
  active: boolean;
  taskId: string | null;
  /** Deep-link event seq (optional). */
  eventSeq: number | null;
}

/**
 * Accepts:
 * - #conversation
 * - #conversation/<taskId>
 * - #conversation/<taskId>/e/<seq>
 * - #conversation/<taskId>?event=<seq>
 */
export function parseConversationHash(hash: string): ConversationRoute {
  const raw = hash.startsWith("#") ? hash : `#${hash}`;
  if (!raw.startsWith(PREFIX)) {
    return { active: false, taskId: null, eventSeq: null };
  }

  const after = raw.slice(PREFIX.length);
  if (after === "" || after === "/") {
    return { active: true, taskId: null, eventSeq: null };
  }

  if (after.startsWith("?")) {
    const params = new URLSearchParams(after.slice(1));
    const eventSeq = parseSeq(params.get("event"));
    return { active: true, taskId: null, eventSeq };
  }

  if (!after.startsWith("/")) {
    return { active: true, taskId: null, eventSeq: null };
  }

  const rest = after.slice(1);
  const qIndex = rest.indexOf("?");
  const pathPart = qIndex >= 0 ? rest.slice(0, qIndex) : rest;
  const query = qIndex >= 0 ? rest.slice(qIndex + 1) : "";

  const segments = pathPart.split("/").filter(Boolean);
  const taskId = segments[0] ? decodeURIComponent(segments[0]) : null;
  let eventSeq: number | null = null;

  if (segments[1] === "e" && segments[2]) {
    eventSeq = parseSeq(segments[2]);
  }

  if (query) {
    const params = new URLSearchParams(query);
    const fromQuery = parseSeq(params.get("event"));
    if (fromQuery != null) eventSeq = fromQuery;
  }

  return {
    active: true,
    taskId: taskId || null,
    eventSeq,
  };
}

function parseSeq(raw: string | null): number | null {
  if (raw == null || raw === "") return null;
  const n = Number(raw);
  if (!Number.isFinite(n) || n < 0) return null;
  return Math.floor(n);
}

export function buildConversationHash(
  taskId?: string | null,
  eventSeq?: number | null,
): string {
  let base = PREFIX;
  if (taskId) {
    base += `/${encodeURIComponent(taskId)}`;
    if (eventSeq != null && eventSeq >= 0) {
      base += `/e/${eventSeq}`;
    }
  }
  return base;
}

export function applyConversationHash(
  taskId?: string | null,
  eventSeq?: number | null,
): void {
  if (typeof window === "undefined") return;
  const next = buildConversationHash(taskId, eventSeq);
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}
