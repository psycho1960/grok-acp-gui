# GAG-019 — UI/UX 精炼（Phase 2）

## 元数据

| 项 | 值 |
|----|-----|
| 分支 | `feat/GAG-017-ui-ux-foundation`（与 Phase 0/1 同分支） |
| 依据 | `docs/UI-UX-OPTIMIZATION-PLAN.md` Phase 2 |
| 依赖 | GAG-017 / GAG-018 |

## 目标

错误可读性、全局命令面板、文案与模式说明、overlay Token 收敛、对比度 CI。

## 范围

- `error-map` + ErrorState 友好文案与复制详情
- Ctrl+K 命令面板（页面跳转 + 任务搜索）
- 模式帮助 Tooltip；PermissionSlot 提交后过渡
- Recovery / Worktree 清理文案
- overlay / border tone Token；`npm run check:contrast`
- **不做**亮色主题（仍需独立 ADR）

## 验收

1. 技术错误（如 ACP handshake）映射为可读标题与建议
2. ErrorState 有「复制错误详情」
3. Ctrl+K 打开命令面板，可跳转页面
4. `npm run check:contrast` 通过并纳入 lint
5. 相关单测通过
