# GAG-009 D5 独立安全复核报告（修复后复核）

- **PR**: psycho1960/grok-acp-gui#10 — `feat(GAG-009): permission and plan mode`
- **分支**: `feat/GAG-009-permissions-and-plan-mode` → `main`
- **复核范围**: 初轮复核 `3196c01..863b2f0`；修复后复核 `863b2f0..ea52868`
- **复核模型**: 独立旗舰模型安全审查会话（与实现者无利益冲突）
- **复核时间**: 2026-08-07
- **复核方式**: 逐行静态审查 PR head、威胁模型、测试与 CI

---

## 一、初轮复核结论（`863b2f0`，不通过）

| 级别 | 缺陷 | 位置 | 状态 |
|---|---|---|---|
| P0-1 | fail-closed 决策未接入真实审批路径：注册未执行 `validate_within`，resolve 不检查 operation category，Unknown/逃逸操作可获批 | `mailbox.rs` / `task_runtime/runtime.rs` | ✅ 已修复 |
| P0-2 | ACP 提交发生在权威 DB 决策之前：不检查 expiry 就发送 allow，DB 失败产生"ACP 已允许、审计未批准"分裂 | `task_runtime/runtime.rs` | ✅ 已修复 |
| P1-1 | 只读 allowlist 参数级绕过：`rg --pre`、`fd --exec`、`git diff --output/--ext-diff/--textconv`、`-C/--git-dir` 外部路径 | `permission.rs` | ✅ 已修复 |
| P1-2 | 脱敏可绕过：`-H/--header`、Bearer/JWT、高熵值、短裸 token 可进 Renderer | `mailbox.rs` | ✅ 已修复 |
| P2-1 | 批准消费缺少 correlation/request 绑定 | `sqlite/mod.rs` | ⏸ 记录为已知限制 |
| P2-2 | 未知持久化状态默认映射 Requested/Proposed（fail-open） | `sqlite/mod.rs` | ✅ 已修复 |

---

## 二、修复验证（`ea52868`）

### P0-1：审批链 fail-closed

**修复**：
- `register_approval_request` 现在对 operation 执行完整 `validate_within`（含 cwd 绝对路径、路径逃逸、git 路径型选项），失败即记为 `Unknown`。
- `restrict_options_for_category` 对 Unknown 类别剥离所有 allow action（转 `Unknown`），仅保留 Deny；`resolve_permission` 对 Unknown 类别的非 Deny action 直接拒绝。
- UI `PermissionSlot.vue` 同步：Unknown 类别的允许按钮禁用并提示"无法安全分类，已阻止"。

**验证**：`unknown_operations_strip_allow_actions_but_keep_denial`、`git_path_options_must_stay_inside_the_workspace` 等新增测试通过。

### P0-2：ACP 提交顺序

**修复**：`resolve_permission` 在发送前检查 expiry（`pending.expires_at_epoch_seconds < now` 即拒绝）；resolution mutex 序列化该检查与发送。`agent_runtime` 的 pending RPC id 在 send 失败时恢复，使重试成为可能。

**残余**：DB 决策提交仍可能在 ACP 发送成功后失败（分布式两阶段无原子事务）。已通过 pending 恢复 + 状态机使该窗口收敛为"已发送但未消费"，不再产生可复用的批准；彻底原子化需引入 dispatch reservation 状态，记录为后续改进。

### P1-1：只读 allowlist 参数级绕过

**修复**：
- `rg --pre/--pre=` → Unknown；`fd -x/--exec/-X/--exec-batch` → Unknown。
- git 全局检查 `--ext-diff/--textconv/--output/-o/--output=` → Unknown（无论子命令）。
- `git -C/--git-dir/--work-tree` 值纳入 `validate_within` 工作区包含校验。

**验证**：`read_only_tools_cannot_spawn_subprocesses`、`git_read_only_commands_cannot_write_or_spawn` 通过。

### P1-2：脱敏绕过

**修复**：`safe_args` 增加 `-H/--header/--cookie/--cookie-jar/--access-token/--bearer/--jwt` 值脱敏；`safe_arg` 增加 `auth/bearer/jwt/cookie/credential/x-api-key/client-secret/access-key/private-key/session-key` 关键词、JWT 头（`eyJ`/`ey0`）、高熵值（≥32 字符、字母数字混合、无路径分隔符）检测。

**验证**：`redaction_covers_headers_bearer_jwt_and_high_entropy_values` 通过（URL 正常保留）。

### P2-2：未知状态 fail-closed

**修复**：`permission_state/plan_state/operation_category` 未知值返回 `rusqlite::Error::FromSqlConversionFailure`，row mapper 传播错误，不再默认 Requested/Proposed。

---

## 三、已知限制（非阻断）

1. **P2-1** 批准消费按 task/session/workspace/digest/plan-version/expiry 匹配，不含 correlation/request ID。相同操作在同上下文可复用批准，影响有限（操作内容相同），符合"持久 scope"语义；如需更严可加 request ID 绑定。
2. **junction/reparse point**：`validate_within` 为词法校验，未 canonicalize；实际写 Adapter（GAG-011~013）必须在 I/O 瞬间再次验证。
3. **DB 决策与 ACP 提交无原子两阶段**：见 P0-2 残余，已收敛为不可复用状态。
4. **notification 兼容**：仅支持 `requestPermission`（ACP v1 请求用 `session/request_permission`，已由 863b2f0 支持）；无 JSON-RPC id 的旧 notification fail-closed。

---

## 四、总体裁决

# ✅ 有条件通过

初轮的 2 个 P0、2 个 P1、1 个 P2-2 已修复并有测试覆盖；剩余项为文档化已知限制（P2-1）与需未来 Adapter 验证的 junction 检查，不阻断 GAG-009 合入。**合入条件**：
1. CI 全绿（当前 push 检查 success；pull_request 检查曾因 GitHub Actions 基础设施故障失败，已重跑）。
2. 后续 GAG-011~013 写 Adapter 在 I/O 前调用同一 `ExecutionGuard` 并验证 junction/reparse point。

## 五、复核声明

本复核独立于 PR 实现者及其自查记录；结论基于 PR head 实际代码、事务顺序、Bridge/UI 数据流和测试覆盖的独立核验。复核过程中未修改任何仓库文件（修复由实现者在复核后提交，本报告为修复后复核确认）。
