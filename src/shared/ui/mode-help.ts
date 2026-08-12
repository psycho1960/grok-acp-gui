/** User-facing mode explanations (Agent / Plan / Ask). */
export const MODE_HELP: Record<string, string> = {
  agent: "智能体：可改文件与执行命令（仍受权限卡约束）。",
  plan: "计划：先规划；批准前阻止写入与非只读命令。",
  ask: "问答：只读问答，默认不在隔离工作区写文件。",
};

export function modeHelpFor(modeId: string | null | undefined): string {
  if (!modeId) return "使用会话或创建时的默认模式。";
  const key = modeId.toLowerCase();
  return MODE_HELP[key] ?? `模式「${modeId}」的行为由 Grok 能力声明决定。`;
}
