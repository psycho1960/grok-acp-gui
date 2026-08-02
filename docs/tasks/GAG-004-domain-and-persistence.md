# GAG-004：领域状态机、SQLite 与启动恢复

## 1. 任务元数据

- Task ID：GAG-004
- 类型：feat/data
- 难度：D4
- 首选模型：DeepSeek V4 Pro，Thinking mode 开启
- 备选模型：GPT-5.6 Sol，reasoning `max`
- 推荐 reasoning effort：DeepSeek Thinking mode；使用 Sol 时为 Max
- 前置任务：GAG-003
- 后续任务：GAG-005、GAG-007、GAG-010、GAG-011

Luna 或 Flash 可生成重复 CRUD fixture 和既定 Schema 下的测试数据，但不得设计 Schema/Migration。低成本模型同一验收项连续失败两次升级 Terra；出现跨表一致性、恢复竞态、Migration 回滚或状态机不变量问题时升级 Sol。

## 2. 目标

实现纯领域状态机与 MOD-PERSISTENCE，使项目、任务、session binding、Worktree、附件、恢复记录和设置能够事务性持久化，并在应用启动时把不可能继续运行的状态恢复为诚实状态。

## 3. 规范映射

- PRD：FR-PROJECT-001/004、FR-TASK-001/003、FR-SESSION-004、NFR-RELIABILITY-002
- UI：UI-TASK-001 的状态、UI-ERROR-001/恢复状态
- 技术：第 4、9 节，MOD-TASK-RUNTIME、MOD-PERSISTENCE

## 4. 必读材料

- PRD 第 4–6 节
- UI 第 5.3、6 节
- 技术第 4、5、9 节
- GAG-003 Bridge DTO 与错误模型

开始条件：GAG-003 契约测试已通过，应用数据目录与 SQLite 版本固定，初始 Schema 尚未被合并发布。若实际基线已存在数据库或 Migration，先盘点版本和数据兼容性，不覆盖或重编号。

## 5. 实现范围

- Project、Task、SessionBinding、WorktreeRecord、AttachmentRecord、RecoveryItem、Settings 领域类型。
- Task/Worktree/Recovery 状态转换函数，非法转换返回领域错误。
- `0001_initial.sql` 与 Migration runner；应用 schema version。
- Rusqlite Adapter、事务、唯一约束、索引和读写 Interface。
- bootstrap snapshot 查询。
- 启动恢复：DB 中 running/waiting_permission/integrating 状态转换为 interrupted；保留错误原因。
- 并发写入串行化/事务策略和数据库损坏错误。

## 6. 非范围

- 不连接真实 ACP、Git 或文件系统。
- 不保存完整消息正文、Token、API Key 或图片二进制。
- 不实现 UI。
- 不加入云同步或多用户表。

## 7. 允许修改

- `src-tauri/src/domain/**`
- `src-tauri/src/modules/persistence/**`
- `src-tauri/src/modules/task_runtime/**` 中纯状态与 bootstrap
- `src-tauri/src/adapters/sqlite/**`
- `src-tauri/migrations/**`
- Bridge 中与既定 DTO 接线的最小实现
- 相应 Rust/contract tests

禁止修改 DesktopBridge 公共命名；确需变更须回到 GAG-003 规范并由 Sol 审查。

## 8. Schema 要求

必须实现技术方案第 9 节七张表及索引。时间统一 UTC ISO/RFC3339 或 integer epoch，项目路径持久化规范化形式与展示形式。外键开启，关键删除采用显式事务而非 cascade 猜测。

Migration：

- 启动时在打开主界面前执行。
- 任一 Migration 失败则整批回滚并返回 `DB_MIGRATION_FAILED`。
- 已合并 Migration 不得修改；测试校验 checksum/顺序。

## 9. 状态机不变量

- Task 只有绑定有效 Workspace 后才能从 preparing 进入 running。
- waiting_permission 必须有关联 request；request 失效后不能保持 waiting。
- merged 不能回到 running；新工作必须创建新任务。
- archived 不自动删除 session、Worktree 或 recovery。
- Worktree state 与 ownership 分离；外部不因“干净”自动变受管。
- Recovery expired 不等于已删除；实际删除成功后才转 deleted。

## 10. 推荐实施顺序

1. 写领域类型和表驱动状态转换测试。
2. 编写 `0001_initial.sql`、Migration runner 和内存临时 DB 测试。
3. 实现 SQLite Adapter 与事务 Interface。
4. 实现 bootstrap 聚合和启动状态修复。
5. 接入 Bridge bootstrap/Task 基础命令。
6. 进行数据库损坏、重复路径和并发写故障测试。

## 11. 自动化测试

- 全状态合法/非法转换表。
- Migration fresh install、重复启动、部分失败回滚和未知较新 schema。
- 唯一约束：project path、session ID、managed worktree path、attachment hash。
- 事务中断不产生孤儿 Task/Binding。
- 启动恢复将 running/waiting_permission/integrating 变 interrupted。
- DB 文件不可写、锁超时、损坏返回稳定错误且不 panic。

## 12. 手工验收

- 启动应用创建 DB；重复启动不重复 Migration。
- 使用测试入口创建项目/任务后重启，列表和状态保留。
- 模拟运行中强退后重启，任务显示“已中断，可恢复”。
- DB 路径和日志不含 Token/图片正文。

## 13. Definition of Done

- Schema、领域状态和 Bridge snapshot 一致。
- 所有写入使用事务或明确的单记录原子操作。
- Migration 锁定且有升级/回滚测试。
- ACP/消息正文/秘密未进入 DB。
- GAG-005/007 可仅通过 Module Interface 消费数据。

## 14. UI、Interface 与外部交互边界

- Repository Interface 必须覆盖 project/task/session/worktree/artifact/recovery/settings 的创建、查询、事务更新和启动快照；调用方不得获得裸 rusqlite connection。
- UI 只通过 GAG-003 的 bootstrap/result DTO 看见稳定状态和 `UI-ERROR-001`；数据库错误不直接暴露 SQL、绝对数据路径或敏感行内容。
- 本任务不调用 ACP、Git、真实项目文件或子进程。路径仅作为经过验证的领域值持久化，不能凭数据库记录获得 I/O 权限。
- 启动恢复只修正本地“进程已不可能继续”的诚实状态，不自动重放命令、写文件或删除 Worktree。

## 15. 异常、恢复与安全不变量

- 数据库 busy、只读、损坏、schema 过新和 Migration 失败均返回稳定错误并保全原文件。
- 事务提交前不得发布对应领域事件；失败事务不留下孤儿关联。
- 运行时配置与秘密分离，密钥不得进入 SQLite、错误详情或日志。
- 已合并 Migration 永不改写；修复只能新增更高序号 Migration。

## 16. 标准任务交付报告

报告必须包含：Task ID；实际模型/reasoning 与升级记录；修改文件；领域状态与 Repository Interface；Schema/Migration 版本、checksum 和升级结果；事务/恢复测试；错误与秘密扫描；测试、Lint、构建退出码；已知兼容性风险。
