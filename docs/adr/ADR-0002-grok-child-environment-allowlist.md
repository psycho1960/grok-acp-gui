# ADR-0002：Grok 子进程采用最小环境变量 Allowlist

- 状态：Accepted
- 日期：2026-08-09
- 任务：GAG-005A

## 背景

GAG-005 原始安全不变量要求子进程环境使用 allowlist，但既有生产 Adapter 实际继承全部父进程环境，再用有限 blocklist 排除已知秘密。未知 API Key、CI Token、云凭据和本地工具配置因此仍可能进入 Grok/ACP 子进程。GAG-005A 明确要求解决该冲突，不能静默保留 blocklist。

## 决策

1. 所有生产 Grok 子进程，包括版本探测、`grok login` 和 `grok --no-auto-update agent ... stdio`，先调用 `env_clear()`。
2. 固定传递下列最小类别：
   - Windows/可执行文件：`PATH`、`PATHEXT`、`SYSTEMROOT`、`WINDIR`、`COMSPEC`。
   - 用户与 Grok 配置目录：`USERPROFILE`、`HOMEDRIVE`、`HOMEPATH`、`HOME`、`APPDATA`、`LOCALAPPDATA`、`PROGRAMDATA`、`GROK_HOME`。
   - 临时目录与语言：`TEMP`、`TMP`、`TMPDIR`、`LANG`、`LC_ALL`、`TERM`。
   - 显式网络与证书：`HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY`、`SSL_CERT_FILE`、`SSL_CERT_DIR`、`CURL_CA_BUNDLE`、`REQUESTS_CA_BUNDLE`。
   - 非秘密 Grok endpoint/OIDC：`GROK_CLI_CHAT_PROXY_BASE_URL`、`GROK_OIDC_ISSUER`、`GROK_OIDC_CLIENT_ID`。
3. API Key 不在固定 allowlist 中。只有当前选择的 Grok model profile 声明了 `env_key` 时，才读取并传递该名称对应的值。
4. 未选择模型、未知模型或 profile 没有 `env_key` 时，不传递任意 API Key；其他模型或云平台密钥也不传递。
5. 环境变量名称必须是最长 128 字节的字母、数字或下划线；非法名称 fail-closed，不进入子进程。
6. 日志和 Renderer DTO 不记录环境变量值、长度、摘要或前后缀；错误只显示缺失变量名称和重启应用动作。
7. 登录与 ACP 使用独立进程生命周期；应用退出会同时取消登录并关闭所有 ACP 进程。

## 结果

- 新增模型 profile 时不需要扩大固定秘密列表；只有当前 profile 的精确 `env_key` 能跨过边界。
- 企业环境如依赖新的代理、证书或非秘密 Grok 配置变量，需要通过单独 ADR 审核后加入固定 allowlist。
- 外部认证 provider 若依赖未列出的自定义环境变量不会被隐式继承；这是安全侧的 fail-closed 取舍。

