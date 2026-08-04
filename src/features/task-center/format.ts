// GAG-007: Presentation formatting helpers (duration, relative time).

export function formatDuration(createdAt: string, updatedAt: string, nowMs = Date.now()): string {
  const start = Date.parse(createdAt);
  const end = Date.parse(updatedAt);
  if (!Number.isFinite(start)) return "—";
  const endMs = Number.isFinite(end) ? Math.max(end, start) : nowMs;
  const seconds = Math.max(0, Math.floor((endMs - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d`;
}

export function formatTimestamp(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return iso || "—";
  try {
    return new Date(t).toLocaleString();
  } catch {
    return iso;
  }
}

export function formatRelative(iso: string, nowMs = Date.now()): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "—";
  const delta = Math.max(0, nowMs - t);
  const seconds = Math.floor(delta / 1000);
  if (seconds < 60) return "刚刚";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}
