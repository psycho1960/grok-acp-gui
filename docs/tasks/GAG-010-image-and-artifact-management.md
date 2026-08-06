# GAG-010：图片与 Artifact 管理

## 1. 任务元数据

| 字段 | 内容 |
|---|---|
| Task ID | GAG-010 |
| 类型 | 多模态 / 文件 I/O / UI 预览 |
| 难度 | D4 |
| 首选模型 | Grok 4.5 |
| 备选模型 | GPT-5.6 Terra |
| 推荐 reasoning effort | High |

Luna 可补充已知 MIME 的预览快照、元数据表格和文档；Flash 可处理确定性的图标/扩展名映射。遇到路径逃逸、超大文件、缓存一致性、恶意内容或 ACP Artifact 语义不明确时升级 GPT-5.6 Sol。

## 2. 背景与目标

用户需要把图片作为上下文发送给 Grok，并查看 Agent 生成或引用的文件。本任务建立受管 Artifact 索引、缩略图、预览和安全打开流程；大二进制不通过通用事件总线传输。

## 3. 需求映射

- PRD：FR-IMAGE-001～005、FR-SESSION-005、NFR-SECURITY-002～004、NFR-PERFORMANCE-002。
- UI：UI-ARTIFACT-001、UI-CONV-001。
- 技术：MOD-ARTIFACTS、ADP-FS、DesktopBridge `import_artifact/list_artifacts/get_artifact_preview/reveal_artifact`。
- 前置：GAG-003、GAG-004、GAG-005、GAG-008；权限集成依赖 GAG-009。

## 4. 必读文档

- `AGENTS.md` 的文件、密钥、日志和 Renderer 边界。
- `01-PRD.md` 3.6、5.7、6。
- `02-UI-UX-DESIGN.md` 5.5、5.8、6～7。
- `03-TECHNICAL-DESIGN.md` 5～6、11～12、14。
- `04-AI-DEVELOPMENT-ROADMAP.md` 阶段 3 与依赖。

## 5. 开始条件

- Artifact DTO、受管根目录和自定义资源协议设计已冻结。
- ACP 能力探测明确是否接受路径、URI 或二进制引用；若不明确须停止报告。
- 最大文件大小、支持 MIME、缩略图尺寸和缓存配额已采用技术方案中的固定值或配置项。

## 6. 实现范围

- 导入图片/文件：选择、拖放、类型嗅探、大小校验、哈希、复制到受管存储。
- 从 ACP `artifact_announced` 建立元数据，验证路径归属。
- Artifact Repository、状态和引用计数。
- 缩略图/预览生成，缓存配额与 LRU 清理。
- Renderer 安全预览：图片、纯文本、受限代码；未知/危险类型仅元数据与“在资源管理器中显示”。
- 会话附件条和 Artifact 面板；加载、处理、失败、缺失、隔离状态。
- 向 Agent 发送稳定 Artifact reference，不把任意本地路径作为可信输入。图片由独立 Luna Runtime 读取受管缓存并生成不可信视觉文本；当前主模型只接收文本，不接收原图。

## 7. 非范围与文件边界

非范围：通用图片编辑、云同步、视频转码、任意文件执行。

> 2026-08-06 用户授权的范围扩展：本次实现输入框图片选择、受管导入，以及固定 `gpt-5.6-luna` 视觉预处理后把纯文本交给当前主模型。该 OCR/视觉描述链路覆盖原“非范围：OCR”；不扩展到通用 OCR 编辑器、云同步或图片编辑。

允许：

- `src-tauri/src/modules/artifacts/**`
- `src-tauri/src/adapters/filesystem/**` 的 Artifact I/O
- `src/features/conversation/**` 的附件入口
- `src/features/review/**` 或专属 Artifact 组件
- 对应 Migration、fixtures、tests

禁止：绕过受管路径从 Renderer 读取任意文件；通过 JSON 事件发送完整二进制/base64；修改 Git Adapter。

## 8. Interface、状态与数据

Artifact 状态：`importing -> ready|rejected|failed`，已存在项可进入 `missing` 或 `quarantined`。状态不得从 failed 自动回 ready，必须重新验证。

`ArtifactService`：`import`、`register_agent_artifact`、`list`、`get_metadata`、`open_preview_stream`、`reveal`、`remove_reference`、`enforce_cache_quota`。

`ArtifactDescriptor` 含 ID、owner task/session、original name、managed relative path、MIME、size、sha256、source、created_at、preview capability、redaction classification。

SQLite：新增 append-only Migration（若未预建）保存 metadata 与引用；二进制在文件系统，数据库不存 blob。删除缓存不删除仍被引用的原件。

## 9. UI 与用户流

导入：拖放/选择 → 本地候选卡 → Bridge 校验与复制 → ready 后可随消息发送；失败显示具体限制并允许移除。

查看：点击时间线 Artifact → 右侧 `UI-ARTIFACT-001` → 元数据、预览、关联任务、显示位置。大文件先显示元数据与显式加载按钮。

安全策略：SVG/HTML/PDF 等主动内容不得在不受控 WebView 中直接执行；未知 MIME 不内嵌。外部打开/显示位置是显式用户动作并通过 Bridge。

## 10. 推荐实施顺序

1. 建受管路径、哈希、MIME 嗅探和限额的纯后端测试。
2. 实现 Repository 与原子导入（临时文件 → 校验 → rename）。
3. 实现预览资源协议和缓存。
4. 实现附件条、Artifact 面板和错误状态。
5. 接入 ACP 引用与权限 Guard。
6. 进行恶意文件、路径和资源耗尽测试。

## 11. 异常与安全不变量

- 所有 canonicalized 路径必须位于批准的受管根或当前受管 workspace；符号链接/重解析点不得逃逸。
- 文件名仅作展示，磁盘键使用生成 ID；禁止路径拼接用户文件名。
- 类型由内容嗅探与扩展名共同判断，冲突时采用更保守策略。
- 超限文件在复制前尽早拒绝；解码设置像素/内存上限，防止图片炸弹。
- 临时导入失败必须可清理；已持久化元数据与文件写入需要可恢复顺序。
- 日志不记录文件正文和完整敏感路径。

## 12. 自动化测试

- Unicode/长文件名、重复内容、同名文件、零字节、超限和类型伪装测试。
- `..`、绝对路径、符号链接/重解析点逃逸测试。
- 原子导入中断、孤儿文件扫描、缺失原件测试。
- 图片炸弹/畸形格式、SVG/HTML 主动内容隔离测试。
- 缓存 LRU、引用计数和配额测试。
- UI 拖放、进度、失败、预览、键盘与大文件按需加载测试。

## 13. 手工验收

1. 导入 PNG/JPEG/WebP 并随消息发送，重启后仍可查看。
2. 拖入超限或伪装文件得到明确拒绝。
3. Agent 生成文件出现在时间线和 Artifact 面板。
4. 删除/移动外部源文件不影响已复制的受管 Artifact。
5. “在资源管理器中显示”只定位受管文件，不执行它。

## 14. Definition of Done

- 导入、索引、预览、缓存、ACP 引用和 UI 流全部完成。
- 路径、类型、大小和主动内容安全测试通过。
- Renderer 不可读取任意路径或接触大型 base64。
- Migration、测试、Lint、类型检查和构建通过。
- 交付报告含支持矩阵、限额、缓存策略、安全验证和模型记录。

## 15. 标准任务交付报告

包含 Task ID、模型/reasoning、修改文件、MIME/大小支持表、Migration、受管目录、测试与手工证据、安全边界、性能数据、已知限制。
