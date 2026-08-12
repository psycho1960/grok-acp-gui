# GAG-020 — UI/UX 微交互与边缘体验（Phase 3）

## 元数据

| 项 | 值 |
|----|-----|
| 分支 | `feat/GAG-017-ui-ux-foundation` |
| 依据 | `docs/UI-UX-OPTIMIZATION-PLAN.md` Phase 3 / 审计 P3 |
| 依赖 | GAG-017–019 |

## 目标

收尾微交互、键盘/触摸可达性、打印与强制清理确认体验，不改变 Bridge 安全契约。

## 范围

- 回到底部：常驻 + 过渡；贴底时隐藏
- Button / IconButton：`:active` 缩放；loading 用 SVG spinner
- Tooltip：300ms 延迟显示
- Skip-to-content + Resizer 键盘提示
- Drawer 右滑关闭（PointerEvent）
- 打印样式（隐藏壳层）
- Worktree 强制清理：UI 输入 `DELETE`，后端仍收真实 `absolutePath`
- 1.5dppx 命中区微调

## 验收

1. `jump-to-bottom` 在 stick-to-bottom 时无 `visible` 类
2. 强制清理需勾选 + `DELETE`；payload.confirmedPath 仍为绝对路径
3. AppShell 含 skip-link 与 resizer tooltip 文案
4. typecheck / lint / 相关单测通过
