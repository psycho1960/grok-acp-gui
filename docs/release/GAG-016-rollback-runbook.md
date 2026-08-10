# GAG-016 Windows 发布与回滚手册

## 发布前检查

1. 固定 release candidate commit，确认工作区、`package-lock.json` 与 `src-tauri/Cargo.lock` 未变化。
2. 运行 `npm run gate:gag015` 和 `npm run release:gag016:verify`。
3. 从 `Windows Package` workflow 选择 `internal-unsigned` 或 `signed`。未配置证书时只能选择 unsigned。
4. 对照 `artifact-manifest.json` 核验版本 `0.1.16`、x64、commit SHA、两个 installer、大小和 SHA-256。signed 候选的每项必须为 `signed-valid`。
5. 将 installer、manifest、checksums 和依赖清单作为同一不可分割发布集合保存。

## 升级验证

1. 复制现有 `app_data_dir` 到独立备份目录，并计算数据库副本 SHA-256；不要移动原目录。
2. 记录原应用版本、数据库 Migration 版本、项目/任务/会话/Artifact 数量及已登记 Worktree/recovery 项。
3. 安装更高版本并启动。确认 Migration 完成且原有记录可访问。
4. 如果 Migration 或数据目录检查失败，立即安全退出；保留原数据、备份与日志，不创建空库替代。
5. 完成后再次记录 schema、计数和数据库 SHA-256，把结果与 candidate manifest 一同归档。

## 回滚触发条件

- installer SHA-256 或 commit SHA 与 manifest 不一致；
- Authenticode 状态不符合候选声明；
- 安装、启动、Migration、数据目录写入或 Grok/Git onboarding 出现阻断错误；
- 升级后项目、任务、会话、Artifact、Worktree 或 recovery 数据不可访问；
- 卸载移除了任何用户数据或工作成果。

## 回滚步骤

1. 停止应用，不删除或重命名当前 `app_data_dir`、用户项目、Worktree、Artifact 或 recovery bundle。
2. 保存失败版本的已脱敏日志、manifest、installer SHA-256 和数据库只读副本。
3. 通过 Windows 设置或原 installer 卸载程序文件。默认卸载不删除用户数据。
4. 若 schema 未升级，安装上一已验证版本并重新检测现有数据。
5. 若 schema 已升级，installer 的降级保护会拒绝旧版本；不要绕过。恢复升级前数据库副本到新的隔离目录，先用兼容版本验证，再由人工决定切换。原数据库继续只读保留。
6. 若 Migration 只完成部分步骤或证据不完整，不启动旧版写入，升级到修复版本或从已验证副本恢复。

## 卸载与重装验证

卸载前后分别检查并记录以下路径/对象是否存在：数据库与设置、`worktrees/`、`recovery/`、工作区 `.grok-acp-gui/artifacts/`、用户项目和外部 Git repository。重装后确认应用重新识别既有数据。任何缺失均停止发布并按上节保存证据。

## 不可执行的回滚动作

- 不修改或回写已合并 SQLite Migration；
- 不用空数据库覆盖失败数据库；
- 不强制安装低版本绕过 schema/installer 降级门禁；
- 不递归删除 AppData、用户项目、外部 repository、Worktree、Artifact 或 recovery root；
- 不从构建日志、工单或发布附件传递证书、密码或 token。
