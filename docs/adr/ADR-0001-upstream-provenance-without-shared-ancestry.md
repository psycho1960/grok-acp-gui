# ADR-0001：上游来源可追溯但不要求共享 Git 祖先

- 状态：Accepted
- 日期：2026-08-02
- 决策者：仓库所有者

## 背景

Grok ACP GUI 从 `formulahendry/acp-ui` v0.1.16（固定 commit
`cd9c3cb464a4b321bff652101953a64c07473e31`）建立产品基线。当前公开仓库先以
`origin/main` 保存规格种子，规格种子与上游仓库没有共同 Git 祖先。

仓库同时规定 `main` 只接受 Squash Merge。即使在任务分支中临时接入上游祖先，
Squash 后上游提交也不会成为 `main` 的祖先；强制改写公开 `main` 又违反现有 Git
安全边界。因此，要求“上游完整历史必须属于产品仓库祖先链”与既定仓库流程不兼容。

## 决策

GAG-001 的上游可追溯性由以下证据共同满足：

1. README、任务交付报告与本 ADR 记录上游仓库 URL、tag 和完整 commit SHA。
2. 导入源码前校验 `v0.1.16` 指向固定 commit，并在交付报告记录命令和退出码。
3. 保留上游 MIT License、原作者版权声明和适用的第三方通知。
4. 开发工作区配置 `upstream` remote；README 记录标准 remote 关系。
5. 产品基线源码从已校验的固定 commit 快照导入，并通过 GAG-001 的删除/保留清单、
   构建、测试和静态扫描验证。

固定上游 commit 不需要成为 PR HEAD 或 `origin/main` 的 Git 祖先。`origin/main`
继续作为产品仓库的规范与交付历史根，所有任务仍通过 Squash Merge 进入 `main`。

## 影响

- 产品仓库的 `git log` 不包含上游完整提交历史；来源与许可证审计依赖上述不可含糊的
  provenance 记录。
- 后续同步上游版本必须创建独立任务，重新固定 URL/tag/commit、比较源码快照并审查
  License 变化，不能声称通过 Git ancestry 自动继承。
- 本决策只适用于 GAG-001 的一次性上游产品基线。它不放宽普通任务分支、受管
  Worktree、Checkpoint、Squash 集成或恢复流程中的共同祖先、路径和引用安全要求。

## 回滚

若未来决定把上游历史纳入祖先链，必须另立 ADR，制定公开仓库迁移、远程引用备份、
协作者通知和回滚方案，并获得对远程历史改写的明确授权。
