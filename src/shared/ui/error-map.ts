/**
 * Map technical backend / bridge messages to user-facing copy.
 * Raw detail is preserved for "复制错误详情".
 */

export type FriendlyError = {
  title: string;
  summary: string;
  suggestion?: string;
  raw: string;
};

type Rule = {
  match: RegExp | string;
  title: string;
  summary: string;
  suggestion?: string;
};

const RULES: Rule[] = [
  {
    match: /ACP handshake failed|handshake failed|EOF/i,
    title: "无法与 Grok 建立会话",
    summary: "桌面壳与 Grok CLI 的会话握手失败。",
    suggestion: "确认 Grok CLI 已安装并已登录，然后重试。",
  },
  {
    match:
      /GROK_AUTH_REQUIRED|not authenticated|unauthenticated|authentication (?:is )?required|login required|grok login|未登录/i,
    title: "Grok 未登录",
    summary: "当前环境无法使用已登录的 Grok 身份。",
    suggestion: "在终端运行 `grok login` 完成登录，然后回到应用恢复会话。",
  },
  {
    match: /grok.*(not found|missing)|command not found|ENOENT/i,
    title: "未找到 Grok CLI",
    summary: "系统 PATH 中找不到可用的 Grok 可执行文件。",
    suggestion: "安装 Grok CLI 并确保可在终端中运行 `grok`。",
  },
  {
    match: /git.*(not found|missing)|not a git repository/i,
    title: "Git 不可用或目录不是仓库",
    summary: "需要 Git，或当前项目未初始化为 Git 仓库。",
    suggestion: "安装 Git，或打开已有仓库；无 Git 时部分集成能力会隐藏。",
  },
  {
    match: /network|offline|ECONNREFUSED|ETIMEDOUT|failed to fetch/i,
    title: "网络或连接中断",
    summary: "与后端或外部服务的连接暂时不可用。",
    suggestion: "检查网络与桌面壳状态后重试。",
  },
  {
    match: /stale|selection.*expired|fingerprint/i,
    title: "选择已过期",
    summary: "文件或状态在操作过程中发生了变化。",
    suggestion: "刷新列表后重新选择，再执行操作。",
  },
  {
    match: /permission|denied|access/i,
    title: "权限不足",
    summary: "当前操作被拒绝或缺少所需权限。",
    suggestion: "检查文件权限与审批选项，或改用更安全的操作。",
  },
  {
    match: /database|sqlite|migration/i,
    title: "本地数据异常",
    summary: "应用数据目录或数据库无法正常使用。",
    suggestion: "重启应用；若仍失败，从恢复中心或诊断页复制信息反馈。",
  },
];

export function mapErrorMessage(
  raw: string,
  fallbackTitle = "出现错误",
): FriendlyError {
  const text = (raw ?? "").trim() || "未知错误";
  for (const rule of RULES) {
    const hit =
      typeof rule.match === "string"
        ? text.toLowerCase().includes(rule.match.toLowerCase())
        : rule.match.test(text);
    if (hit) {
      return {
        title: rule.title,
        summary: rule.summary,
        suggestion: rule.suggestion,
        raw: text,
      };
    }
  }
  return {
    title: fallbackTitle,
    summary: text.length > 160 ? `${text.slice(0, 160)}…` : text,
    suggestion: "可复制错误详情以便反馈。",
    raw: text,
  };
}

export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* fall through */
  }
  try {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.left = "-9999px";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(area);
    return ok;
  } catch {
    return false;
  }
}
