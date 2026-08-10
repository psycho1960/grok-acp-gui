# GAG-016 Windows 打包、安装与升级交付报告

## 一、任务完成情况

- Task ID：GAG-016。
- 已完成：创建 `feat/GAG-016-windows-packaging`；冻结 0.1.16 Windows x64 NSIS/MSI metadata 与 MSI UpgradeCode；标准用户 NSIS、installer 降级拒绝、WebView2 bootstrapper；unsigned/signed CI 接口；临时证书清理；真实 bundle 构建；SHA-256/签名状态/依赖清单；安装/卸载保留 smoke 脚本；v6 数据库副本升级与更高 schema 拒绝测试；发布说明与回滚手册。
- 未完成：没有签名证书和时间戳服务，未生成已签名候选；当前机器不是干净/临时 runner，未实际执行会修改 Windows installer 注册和 AppData 的 smoke 脚本；干净 VM 标准用户手工验收未执行；GAG-015 已记录的外部审查与原生手工证据未由本任务扩展或代做。
- 与计划差异：升级拒绝测试发现 `SqliteRepository::open` 在 schema 门禁前切换 WAL、会修改更高版本数据库文件头；按 GAG-016 明确的降级 fail-closed 要求，仅调整为 Migration 成功后启用 WAL。无 Interface、状态机或 schema 变化。
- 实际使用模型及升级/降级：GPT-5.6 Sol（当前会话模型），高于任务首选 GPT-5.6 Terra；因平台会话已固定使用，未触发额外模型升级，也未执行用户明确不要求的独立安全审查。

## 二、构建环境与产物

- Windows 11 专业版 `10.0.22631` x64。
- Node `v24.15.0`、npm `11.12.1`、Rust/Cargo `1.97.1`、Git `2.51.0.windows.1`。
- target：`x86_64-pc-windows-msvc`；版本 `0.1.16`；identifier `com.grokacpgui.desktop`。
- `Grok ACP GUI_0.1.16_x64_en-US.msi`：5,242,880 bytes；SHA-256 `0518cd5dbd3817efde5b8dfb5f103ce7da6d8b6949f4659082af74a32844db05`；Authenticode `NotSigned`。
- `Grok ACP GUI_0.1.16_x64-setup.exe`：3,658,022 bytes；SHA-256 `ec333b3af7cbf1137e3e9eaa5a11b048fc4751b34eaeec6f62400b7873873dbb`；Authenticode `NotSigned`。
- 候选分类：**internal unsigned candidate**，不得声称 production-ready 或已签名。
- 本地忽略证据：`.gag-016-evidence/artifact-manifest.json`、`checksums.sha256`、npm/Cargo dependency inventory。CI 将相同文件与 installer 一起保存 14 天。

## 三、修改文件与 Interface / 数据变化

- 新增：Windows packaging workflow；release/manifest、signing、installer smoke 脚本；Node/Rust GAG-016 测试；发布说明、回滚手册与本报告。
- 修改：`src-tauri/tauri.conf.json`、`package.json`、`.gitignore`、SQLite Adapter 启动 pragma 顺序。
- 删除：无。
- DesktopBridge/事件/DTO：无变化。
- SQLite Migration：无新增、无修改；既有 0001～0008 checksum 不变。
- 状态机：无变化。
- 配置：NSIS/MSI only、x64 only、publisher/license/description/icon、NSIS `currentUser`、MSI frozen UpgradeCode、`allowDowngrades=false`、SHA-256 signing digest、WebView2 download bootstrapper。
- 回滚方式：恢复上述配置、脚本、workflow 和文档；SQLite Adapter 可恢复原 pragma 顺序，但会重新引入更高 schema 数据库被 WAL header 修改的问题。没有业务数据 Migration 回滚。

## 四、测试结果

- GAG-016 配置/manifest Node 测试：2/2，通过；覆盖空格/Unicode 路径、NSIS+MSI、SHA-256、缺失 MSI 和 unsigned 拒绝 signed 声明。
- GAG-016 Migration 集成测试：2/2，通过；v6 原库 SHA-256 保持不变、只升级副本到 v8；schema 99 返回 `DB_MIGRATION_FAILED`，文件 SHA-256 与 version 99 均保持不变。
- GAG-015 release gate：退出码 0，12/12 步骤通过。Node 34/34、Vitest 212/212、Playwright 34/34、Rust unit 226/226 与全部 GAG-005～016 integration families 通过；typecheck、lint、static gate、Rust fmt/check/clippy、frontend build、Tauri build 通过。
- Tauri x64 release build：退出码 0，真实生成 NSIS 与 MSI；bundle metadata 和本地 Authenticode/SHA-256 manifest 通过。
- ACP 契约测试：由 GAG-015 Fake ACP/Bridge 测试继续覆盖；本任务未改 ACP。
- Git 集成测试：由 GAG-015 临时仓库测试继续覆盖；本任务未改 Git/Worktree Interface。
- PowerShell：两个脚本 AST 解析通过；实际签名 prepare/cleanup 因无证书 secrets 未执行。
- Windows installer smoke：已实现并接入只允许显式 ephemeral runner 的 CI；本机未执行，GitHub workflow 尚未触发。
- 手工验收：未执行干净 Windows VM、真实标准用户启动、签名显示、高 DPI/屏幕阅读器；不虚构通过。

## 五、安装、升级与数据保留矩阵

| 场景 | 自动化状态 | 结果/证据 |
|---|---|---|
| x64 NSIS/MSI release bundle | 本机执行 | 通过，两个真实产物与 manifest 齐全 |
| NSIS 静默首装/卸载/重装 | CI 脚本就绪 | 待临时 Windows runner 执行 |
| MSI 静默首装/卸载 | CI 脚本就绪 | 待临时 Windows runner 执行 |
| 数据目录含空格/Unicode | Node/Rust fixture | 通过 |
| v6 → v8 覆盖升级 | Rust 副本 fixture | 通过，原数据库保留且 hash 不变 |
| 更高 schema 降级拒绝 | Rust schema 99 fixture | 通过，拒绝且不修改数据库 |
| 卸载保留 DB/Worktree/recovery/repo/Artifact | CI sentinel 断言就绪 | 待临时 Windows runner 执行 |
| Authenticode 签名与时间戳 | 接口与 fail-closed manifest 就绪 | 无 secrets，未执行；当前产物 NotSigned |

## 六、风险与发布建议

- 风险：签名链与时间戳未验证；installer lifecycle 尚无本次 CI/干净 VM 运行证据；GAG-015 前置报告仍明确“不放行”。
- 对已有功能的影响：只把 WAL 切换延后到 Migration 成功之后；正常数据库仍使用 WAL，更高 schema/失败 Migration 不再产生持久 journal-mode 写入。
- 性能与安全注意事项：未新增 Renderer 权限、shell/文件系统直通、自动更新或遥测；签名 secrets 仅进入临时进程环境/证书库，cleanup 使用精确 thumbprint receipt。
- 发布建议：可作为内部无签名候选供临时 runner/干净 VM 验证；完成 signed workflow、installer smoke 与手工矩阵前不得标记生产候选。

## 七、后续事项

- 明确未完成项：触发 `Windows Package` workflow；在有授权证书时验证 signed 分支；干净 VM 标准用户首装/升级/卸载/重装与数据可访问性；按 GAG-015 自身范围补齐其历史/外部审查门禁。
- 建议下一步：由发布负责人在临时 Windows runner 先执行 `internal-unsigned` workflow，核对上传的 manifest/checksum/installer smoke；证书到位后再单独执行 `signed`。
