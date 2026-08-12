# GAG-018 — UI/UX 控制台成型（Phase 1）

## 元数据

| 项 | 值 |
|----|-----|
| 分支 | `feat/GAG-017-ui-ux-foundation`（与 Phase 0 同分支迭代；合入前可 squash 为两提交） |
| 依据 | `docs/UI-UX-OPTIMIZATION-PLAN.md` Phase 1 |
| 依赖 | GAG-017 Phase 0 Token / Toast / Icon |

## 目标

降低 TaskCenter 信息噪音，补齐控制台导航与状态可观察性，提升可发现性与动效质感。

## 范围

- TaskCenter 头部：搜索常驻、筛选折叠、项目菜单、去掉重复 chip 行
- Shell 左栏分组 + 图标 + 激活竖条；顶栏面包屑；状态栏三区信息
- Dialog/Drawer 入场动画
- Composer placeholder 简化、快捷指令 `?`、快捷键帮助
- 首次引导浮层
- Review 返回链、Checkpoint 进度、窄屏折叠
- VirtualList `aria-rowcount` / `aria-posinset`

## 不在范围

- Ctrl+K 命令面板、亮色主题、错误映射大表

## 验收

1. TaskCenter 默认不展示完整 filter 栅格；`toggle-filters` 可展开
2. 左栏导航有分组标题与激活态
3. 顶栏面包屑随路由变化
4. 状态栏含路径 / session / model 区域
5. Composer 默认 placeholder 为「输入消息…」
6. 相关单测通过
