# Pi Agent MVP：安全边界与协议

## 范围

Miao 的 Pi Agent 是一个按需启动、一次性、纯文本助手。MVP 只支持 glibc Linux x86_64/aarch64 和 API Key Provider；不支持 OAuth、工具、扩展、技能、会话持久化或原生 OpenWrt/musl。

## 信任边界

- **受信任**：运行 Miao 的操作系统管理员、Miao 后端、固定版本且校验通过的 Pi 二进制。
- **不受信任**：浏览器输入、模型输出、网页 Origin、下载网络和 Provider 错误文本。
- Provider 会收到用户消息和固定系统提示；用户不应在聊天中发送订阅链接、代理凭据或其他密钥。

MVP 不把 Pi 当作完整的内核级沙箱。Pi 仍需要访问 Provider 网络；如果 Pi 或其运行时本身被利用，临时低权限 UID 只能降低而不能消除主机风险。以非 root 用户运行 Miao 时，Pi 与 Miao 使用同一 UID。

## 浏览器边界

助手端点为：

- `GET /api/agent/status`
- `POST /api/agent/config`
- `GET /api/agent/ws`

所有端点都要求 TCP peer 为 loopback。配置和 WebSocket 还要求 `Host` 为 loopback authority，并在浏览器提供 `Origin` 时要求 Origin 与 Host 完全匹配。这用于阻止局域网访问、跨站 WebSocket、CSRF 和 DNS rebinding。它不是 Miao 整体面板的认证机制。

浏览器不能发送任意 Pi JSONL。Miao 只接受以下 WebSocket 消息：

```json
{"type":"prompt","message":"..."}
{"type":"abort"}
```

单条浏览器消息最多 64 KiB，单个 prompt 最多 8,000 个 Unicode 字符。后端只生成白名单 Pi RPC 命令；即使 Pi 的 RPC 支持 `bash`，浏览器也无法请求该命令。

后端只向浏览器转发纯文本增量、完成文本、阶段、重试提示和已脱敏错误。任何 `tool_execution_*` 或 `bash_execution_*` 事件都会立即终止会话。Pi JSONL 记录在读取过程中限制为 4 MiB，避免先无限分配后再校验。

## 凭据

Provider API Key 保存在 Miao 配置文件同目录的 `.miao-agent/credentials.json`：

- 目录权限 `0700`，文件权限 `0600`
- 原子替换，不写入普通 `config.yaml`
- 不写入 Pi 的临时 `settings.json`、日志、命令行或浏览器响应
- 只通过对应 Provider 的环境变量注入子进程
- Provider 原始错误不会直接返回浏览器

密钥仍会存在于 Miao 和 Pi 的进程内存中，并可被主机 root 读取；这是本地 API Key 模式的剩余风险。

## 下载与执行

Miao 固定 Pi 版本、架构、压缩包大小、解压后二进制大小和 SHA-256。下载采用唯一且排他创建的临时文件，最多重试三次；只从校验后的归档提取二进制和三个必需主题文件，忽略其他内容。

首次安装前检查：

- glibc 动态加载器
- `/tmp` 的 `noexec` 标志
- 至少 256 MiB 可用空间
- 至少 64 个可用 inode
- 至少 512 MiB `MemAvailable`

缓存位于 `/tmp/miao-pi-agent`。每个会话使用独立私有 HOME、工作目录、配置目录和 TMPDIR。环境默认清空，只恢复 Provider 所需密钥、代理/证书变量和少量固定运行变量。启动参数禁用工具、扩展、技能、模板、主题、上下文文件、审批、更新检查和会话持久化。

Miao 以 root 运行时，会选择未在 `/etc/passwd` 或 `/etc/group` 登记的临时低权限 UID/GID，并清除补充组后启动 Pi；无法使用该身份时才回退到 nobody。同一 Miao 实例最多运行一个助手会话；会话关闭、浏览器断开、十分钟空闲超时、异常工具事件或 Miao 退出都会终止 Pi，并清理会话目录。

## 已知限制

- 官方 Pi Linux 资产依赖 glibc，Miao 的 musl/OpenWrt 发布包不代表 Pi 可在 musl 上运行。
- 临时 UID、空环境和禁用工具不是 seccomp、Landlock、容器或虚拟机级隔离。
- 本地恶意进程仍可调用 loopback API；MVP 将本机用户视为信任边界的一部分。
- 多 Provider、OAuth、受限诊断工具和远程访问需要单独的认证、TLS 与沙箱设计，不能直接复用当前边界。
