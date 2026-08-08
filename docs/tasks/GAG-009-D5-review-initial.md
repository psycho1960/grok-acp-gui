# GAG-009 D5 独立安全复核报告（初轮，863b2f0）

- **复核范围**：PR #10，`origin/main@f24f41c` 至 `feat/GAG-009-permissions-and-plan-mode@863b2f0`，共 36 个文件、2 个提交。
- **复核模型**：独立旗舰模型安全审查会话（与实现者不同模型）。
- **复核时间**：2026-08-07。
- **复核方式**：逐行静态审查 PR head、威胁模型、测试与 CI；未修改或切换工作树。
- **修复状态**：本报告为初轮结论；修复见 `GAG-009-D5-review.md`（修复后复核，有条件通过）。

## 一、逐项结论表

| # | 检查项 | 结论 | 证据与判断 |
|---|---|---|---|
| 1 | fail-closed | **缺陷** | `ExecutionGuard` 本身调用 `validate_within` 并拒绝 Unknown，但权限落库只调用 `category()` 未调用 `validate_within()`；resolve 仅拒绝 Unknown option action，未拒绝 Unknown operation category。未知命令、缺失 cwd、路径逃逸可搭配 `allow_once` 提交给 ACP。UI 也只检查 option kind。 |
| 2 | 批准证据绑定 | **风险** | digest 覆盖 kind/executable/argv/cwd/全部读写路径；消费 SQL 校验 task/session/workspace/digest/plan version/state/expiry。但 `ExecutionContext` 和消费查询不含 correlation/request ID，同上下文同 digest 的批准可被另一请求消费。 |
| 3 | 竞态与单次消费 | **缺陷** | mutex + SQLite 条件 UPDATE 阻止普通双击，`approved_once` 消费原子。但后端先发送 ACP allow 再检查 expiry 并提交 DB 决策；过期请求或 DB 提交失败时 ACP 已收到允许。pending map 在 outbound send 前移除，失败后无法安全重试。 |
| 4 | 敏感参数 | **缺陷** | 脱敏规则为关键词/长度启发式；`--header` 后 Bearer 值、`-H "X-API-Key: <值>"`、短裸 Token 可原样进入 session event 和 Renderer。option label、工具标题、cwd、路径未经同等脱敏。 |
| 5 | 路径逃逸 | **缺陷** | 词法规范化可处理分隔符/大小写/`..`/盘符，但不 canonicalize，不检查 junction/reparse point；审批注册链根本未调用路径校验。Git `-C/--git-dir/--work-tree` 值仅被跳过，未验证 workspace。 |
| 6 | ACP 契约 | **风险** | JSON-RPC raw id 与 pending map 关系一致；响应原样发送 option ID；未知 kind 拒绝；无 id notification fail-closed。风险：map key 优先用 ACP 自报 `requestId`，重复可覆盖；notification 仅兼容 `requestPermission`。 |
| 7 | Renderer 边界 | **风险** | Renderer 只提交 task/session/request/correlation/version/optionId，无审批令牌/digest/scope/Shell。边界结构正确，但 argv 脱敏不完整。 |
| 8 | 恢复语义 | **风险** | 取消/进程退出/启动恢复 expire pending/approved_once 并使 proposed Plan 失败。`approved_scope` 在恢复后继续存在，缺少重启后重新判定。 |
| 9 | 审计 | **通过** | 审计表只保存元数据与脱敏摘要，未写 argv、环境变量或原始 operation。 |

## 二、缺陷清单（初轮）

### P0-1：核心 fail-closed 决策未接入真实审批路径
Unknown operation、缺失 cwd、工作区外路径、Plan 未批准的写操作可携带 `allow_once` 被批准；全仓无 `ExecutionGuard::authorize` 生产调用点。

### P0-2：ACP 提交发生在权威 DB 决策之前
已过期请求或 DB 失败时 ACP 已收到允许；pending RPC 映射在 send 前移除，失败无法重试。

### P1-1：只读命令 allowlist 参数级绕过
`rg --pre`、`fd --exec` 可启动子进程；`git diff --output=` 可写文件、`--ext-diff/--textconv` 可启动外部程序；`-C/--git-dir/--work-tree` 可指向 workspace 外。

### P1-2：Renderer/session event 参数脱敏可绕过
短裸 Token、分离 Bearer header、未识别 `X-API-Key` 等会持久化并展示。

### P2-1：批准消费缺少 correlation/request 绑定
同上下文同 digest 的另一请求可消费先前批准。

### P2-2：未知持久化状态映射为可处理状态
未知 permission state → Requested、未知 Plan state → Proposed（fail-open）。

## 三、总体裁决（初轮）

# 不通过

存在两项 P0：实际 ACP 审批链允许 Unknown/逃逸/未批准 Plan 操作，且在后端权威决策与过期校验前就向 ACP 发送允许。`ExecutionGuard` 局部实现与 SQLite 单次消费逻辑较完整，但未形成不可绕过的生产执行边界，不满足 GAG-009 fail-closed 门禁。PR #10 当时不应合并。

## 四、修复对照

| 缺陷 | 修复提交 | 修复方式 |
|---|---|---|
| P0-1 | `ea52868` | 注册执行完整 `validate_within`；Unknown 类别剥离 allow action；resolve 拒绝 Unknown 类别非 Deny；UI 同步禁用 |
| P0-2 | `ea52868` | resolve 发送前检查 expiry；pending RPC id 发送失败时恢复 |
| P1-1 | `ea52868` | `rg --pre`/`fd --exec`/git `--output`/`--ext-diff`/`--textconv` → Unknown；git 路径型选项纳入包含校验 |
| P1-2 | `ea52868` | `-H/--header`/Bearer/JWT/高熵值脱敏 |
| P2-1 | 记录为已知限制 | 消费匹配不含 correlation/request ID（文档化） |
| P2-2 | `ea52868` | 未知状态返回解码错误 fail-closed |

## 五、复核声明

本复核独立于 PR 实现者及其自查记录；威胁模型仅作为验收依据，结论来自 PR head 实际代码、事务顺序、Bridge/UI 数据流和测试覆盖的独立核验。复核过程中未修改任何仓库文件。
