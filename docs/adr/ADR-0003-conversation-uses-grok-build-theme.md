# ADR-0003：对话面采用当前 Grok Build 主题（Rose Pine Moon）

- 状态：Accepted
- 日期：2026-08-13
- 任务：GAG-021

## 背景

`docs/02-UI-UX-DESIGN.md` 与 UI-SETTINGS 规定 v1 固定 Catppuccin Mocha。对话体验 grilling 中，用户认为 Mocha 用法偏灰，并要求直接使用当期 Grok Build 主题色。本机 `~/.grok/config.toml` 为 `theme = "rosepine-moon"`，与用户提供的 Grok TUI 截图一致。Grok Build 出厂默认是 GrokNight，不是 Rose Pine Moon。

## 决策

1. 对话面（时间线、任务条、Composer、Conversation rail、历史轮次菜单）的色源是 **Rose Pine Moon** 官方色板，不是 Catppuccin Mocha。
2. 代表色：base `#232136`，surface `#2a273f`，overlay `#393552`，text `#e0def4`，iris `#c4a7e7`，love `#eb6f92`，foam `#9ccfd8`，gold `#f6c177`。
3. 本期不跟 TUI 做运行时主题同步，不做亮色主题，不引入主题选择器。
4. 任务中心、审查、恢复等未纳入本期的页面，可暂留 Mocha，直到单独任务迁移。
5. 修正 `docs/02-UI-UX-DESIGN.md` 中「对话面固定 Mocha」的表述，指向本 ADR。

## 结果

- 对话与用户日常使用的 Grok Build TUI 同色温，减少「后台灰壳」感。
- 与既有 Token CI（对比度、禁止散落品牌色）仍兼容：换的是色板值与用法，不是取消 Token。
- 若日后要跟随 TUI 的 `/theme` 切换，需要新的 ADR 和设置面。
