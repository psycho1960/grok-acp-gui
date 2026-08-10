# Grok ACP GUI 0.1.16 Windows 内部候选说明

## 候选定位

GAG-016 首版仅发布 Windows 10/11 x64 的 NSIS 与 MSI。默认构建是 **internal unsigned candidate**；只有 CI 选择 `signed`、签名与时间戳验证通过且 manifest 标记为 `signed-production-candidate` 时，才可称为已签名候选。本任务不启用自动更新、不发布 Microsoft Store 包，也不下载 Grok Build。

## 安装包选择

- NSIS：默认 `currentUser`，标准用户无需管理员权限，适合个人安装。
- MSI：面向需要 Windows Installer 管理的环境，静默安装可能需要管理员权限。
- 两者均为 `x86_64-pc-windows-msvc`；ARM64/x86 不在本期范围。
- installer 在需要时通过 Microsoft bootstrapper 安装或更新 WebView2。应用自身不会静默下载 Grok Build。

## 安装与运行数据归属

- 程序文件：由 NSIS/MSI 放入 Windows 认可的安装位置，不保存用户项目或可变业务数据。
- 应用数据：Tauri 的版本无关 `app_data_dir`，Windows 下对应 roaming AppData 中由 `com.grokacpgui.desktop` 标识派生的目录；其中包含 `grok_acp_gui.db`、`worktrees/` 与 `recovery/`。
- Artifact：位于任务工作区的 `.grok-acp-gui/artifacts/`，不复制到程序安装目录。
- 用户项目与外部 Git repository：保持原位置，installer 不接管、不移动。
- 日志与诊断：沿用应用现有诊断输出；发布包不新增遥测或上传。

默认卸载只移除程序与 installer 注册信息，不删除上述数据库、设置、Worktree、Artifact、recovery bundle、用户项目或外部 repository。当前 installer 不提供“同时删除用户数据”选项。

## 首次启动与升级行为

- 首次启动继续使用既有 onboarding 检测 Git、Grok、Grok 版本/认证、数据库与数据目录可写性。
- Grok 缺失或路径变化时只提供手动路径、重新检测与诊断，不自动执行未知下载。
- 应用启动时执行仓库内既有不可变 SQLite Migration。Migration 或数据目录初始化失败时 bootstrap 返回阻断状态，不进入可写主流程。
- installer 设置 `allowDowngrades: false`，阻止用更低应用版本覆盖更高版本；应用的既有 schema 检查仍是数据库写入的最终门禁。
- MSI UpgradeCode 固定为 `59b1c3c7-7027-5376-86b5-69993d342750`，后续版本不得变更。

## 产物与验证

CI 的 `Windows Package` workflow 固定 x64 target，并输出：

- NSIS `.exe` 与 MSI `.msi`；
- `artifact-manifest.json`：版本、架构、commit SHA、文件大小、SHA-256 与 Authenticode 状态；
- `checksums.sha256`；
- npm 与 Cargo 完整依赖清单。

签名证书、密码与时间戳 URL 只从 CI secrets 注入临时用户证书库和 runner 临时配置；清理步骤始终执行。仓库与构建日志不写出 PFX、密码或 token。

## 已知限制

- 当前仓库没有签名证书或发布渠道配置，因此本地生成物只能称为内部无签名候选。
- GAG-015 报告记录的独立模型审查、干净 VM 原生手工旅程、高 DPI/屏幕阅读器与长时性能证据仍未完成；本任务不扩展或代做这些审查。
- GitHub-hosted Windows runner 的静默安装 smoke test 不是干净 Windows VM 手工验收的替代品。
- 无远程自动更新与自动回滚服务；回滚按配套 runbook 人工执行。
