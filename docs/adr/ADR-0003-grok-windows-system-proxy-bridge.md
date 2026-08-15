# ADR-0003：把 Windows 静态系统代理桥接到 Grok 子进程

- 状态：Accepted
- 日期：2026-08-15
- 任务：GAG-005 / UI-ONBOARD-001 修复

## 背景

Windows 用户可能只在 Internet Settings 中启用系统代理，而不设置
`HTTP_PROXY`、`HTTPS_PROXY` 或 `ALL_PROXY`。GAG 自身的网络预检能够读取
系统代理，但 Grok 1.0.3 的 OAuth HTTP 客户端在这种环境下无法访问
`auth.x.ai`，导致登录进程持续重试 discovery 且不会打开浏览器。

## 决策

1. 继续以 ADR-0002 的最小环境 allowlist 为安全边界。
2. 若父进程已显式提供任一代理环境变量，原样使用，不读取或覆盖系统代理。
3. 仅在 Windows 且没有显式代理环境时，读取当前用户 Internet Settings 中已启用的静态 `ProxyServer`。
4. 将有效的 HTTP、HTTPS 或 SOCKS 静态代理转换为 Grok 已允许接收的 `HTTP_PROXY` 和 `HTTPS_PROXY`；同时补充只包含 loopback 的 `NO_PROXY`。
5. 不解析 PAC 脚本，不记录、展示或持久化代理地址及凭据。无效或不支持的代理配置 fail-closed，不进入子进程环境。
6. 该回退同时用于版本探测、OAuth 登录和 ACP 进程，避免启动检查与真实运行使用不同网络路径。

## 结果

- 仅配置 Windows 静态系统代理的用户可以完成 Grok OAuth 登录和后续 ACP 网络请求。
- 显式环境变量仍具有最高优先级，企业现有部署行为不变。
- PAC-only 环境仍需显式提供代理环境变量；应用会显示认证服务不可达，而不会猜测或执行 PAC。
