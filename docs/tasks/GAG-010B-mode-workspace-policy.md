# GAG-010B：模式与工作区策略联动及 Fail-Closed 约束

## 目标与依赖

本任务依赖 GAG-008～010A 已有的会话、权限、持久化和 DesktopBridge 契约，在 GAG-011 实现 Worktree 生命周期前收口策略选择与运行时安全边界。路径解析涉及真实文件系统时按 D5 处理。

## 映射与 Interface

- 默认映射唯一为 `ask -> direct`、`agent|plan -> worktree`；未知或旧记录采用 `worktree` 安全默认。
- 用户可显式覆盖为 `worktree`、`readonly`、`direct`，其他值由 Bridge 返回 `BRIDGE_VALIDATION_FAILED`。
- `session.configure` 将同一次 mode/strategy 修改作为一个 TaskRuntime 配置补丁保存；`task.open` 返回持久化的 `workspaceStrategy` 与后端验证的 `workspaceAvailable`。
- Renderer 只调用 DesktopBridge，不推导真实 cwd；Create Task 与 Conversation Header 复用同一前端映射。

## Fail-Closed 不变量

- Worktree 记录缺失、非受管、状态不可用、路径不存在、仓库身份不匹配、路径等同或嵌套原项目时返回 `WORKTREE_NOT_READY`。
- Worktree 策略绝不回落到 `project.path`；失败前不得启动 ACP 或写入工作区。
- 配置与 Turn 启动按 task 串行；运行中的 Turn 不切换 cwd。retained ACP 的已绑定 cwd 与重新解析 cwd 不一致时关闭并拒绝复用。
- `readonly` 使用项目目录但权限分类保持只读，写入与未知操作不能获得 allow 选项。
- UI 仅在后端成功后更新稳定设置；失败时恢复后端状态，保存期间禁止发送。

## 非范围

不创建、接管、删除、合并或清理 Worktree，不执行任何 `git worktree` 命令，不实现 GAG-011 的 managed-root 配置和 Git 生命周期。

## 验收与测试

覆盖默认/覆盖/非法策略、原子保存、重开与 SQLite 重启、跨任务隔离、旧记录安全默认、缺失或伪造 Worktree 零 ACP/零写入、retained cwd 漂移、运行态切换拒绝、direct 重新绑定和 readonly 权限负面路径。质量门禁遵循仓库 AGENTS.md。
