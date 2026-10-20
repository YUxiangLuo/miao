# 配置参考

启动时优先读取可执行文件旁的 `config.yaml`，否则使用[平台默认路径](profiles.md#文件位置)。文件不存在也能启动，可在面板完成配置。面板无鉴权，访问限制见[平台说明](platforms.md)。

## 命令行启动

```bash
sudo ./miao --config /path/to/config.yaml
sudo ./miao --sub 'https://example.com/sub?token=xxx' JP
./miao --help
./miao --version
```

示例假定二进制名为 `miao`。`--config=PATH`、`--sub=URL` 同样有效，两者互斥；参数在提权、写文件和启核前校验。

`--sub` 接受 HTTP/HTTPS Clash YAML 订阅。地区可选 `HK/JP/TW/SG/US`，大小写不限；省略时手动选择，无地区候选时临时回退手动。临时运行默认规则分流、不限倍率，不继承已有配置或偏好，面板修改也仅影响本次运行；清理规则见 [Profile](profiles.md#sdk-与生命周期)。

订阅 URL 须加引号，令牌会进入 shell 历史和进程参数。**不支持多开**，TUN 和 Clash API 仍共享。每个订阅响应最多 4 MiB，原始 `proxies` 最多 10000 项（含被过滤节点）；超限按解析失败处理，保留已有缓存，不截断或当作成功空列表。

## 配置文件

```yaml
port: 6161
subs:
  - "https://your-subscription-url"
nodes:  # sing-box outbound JSON
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
custom_rules:
  - '{"domain_suffix":"example.com","action":"route","outbound":"direct"}'
  - '{"process_name":"qbittorrent","action":"route","outbound":"HY2"}'

# 可省略；持久偏好优先于策略/倍率的启动默认值
mcp: false
node_select: manual  # 或 fastest_hk、fastest_jp、fastest_tw、fastest_sg、fastest_us
max_multiplier: null  # 不限，或正数（如 2.5）
route_mode: rule  # rule / global
scheduled_refresh:
  enabled: false
  times: ["04:30", "16:30"]  # 运行主机的本地时区，最多 24 个
```

节点支持范围见[内核能力](kernel.md#能力与裁剪)。自定义规则优先于内置分流，全局模式下仍生效；出口可填 `proxy`、`direct` 或节点 tag。引用缺失/禁用节点的规则会在原位置改为阻断，保留匹配条件，不回退到后续直连或默认代理。原规则保留，原节点恢复后自动恢复原出口；面板显示阻断标记。

阻断由运行中的内核执行；主动停服或删除最后一个节点来源导致停服后，自定义规则无法拦截系统流量。

## 状态保存与节点选择

稳定配置、易变状态和偏好分别保存，文件用途与位置见 [Profile](profiles.md#文件位置)。地区回退为 `manual` 不覆盖用户请求的策略，后续刷新继续尝试；具体手动节点由 `.last_proxy` 恢复。

最高倍率只筛选地区最快模式，手动选择保留完整节点池。倍率取自当前显示名，如 `18x`、`6.5X`、`2.4倍`、`倍率：1.3`；未标记按 1x，无效数值不进入受限候选。选项来自完整节点池，“不限”恢复全部候选，偏好文件用 `unlimited` 表示。

### 开机订阅刷新失败

有兼容缓存、订阅快照或手动节点时，先用本地材料启核，再后台刷新。首轮最多 20 秒，随后最多 4 次快速重试，间隔 5、10、20、40 秒（另加请求耗时）。代理已就绪但拉取仍失败时，保留运行配置并告警，转为每 30 分钟静默重试；代理未就绪则继续恢复。

已配置订阅 URL 的主机在分流和全局模式下默认直连，域名解析也使用直连 DNS，避免失效的默认节点阻断订阅更新。这项规则位于自定义规则之后；需要代理访问的订阅可用自定义规则指定出口，出口缺失时仍按阻断处理。自动直连仅覆盖订阅 URL 中的主机，不包含跨主机重定向的目标。

手动刷新失败不重置快速重试额度，接受响应并完成提交后结束启动恢复；停服或修改订阅取消旧任务。成功空列表不算网络失败：有替代节点则应用并删除旧节点，无候选但代理可用则保留运行态并提示检查配置。缓存可用不代表拉取成功，后台拉取也不改变代理就绪状态。API 语义见[订阅刷新](runtime-state.md#订阅刷新)。

### 定时刷新订阅

在订阅管理点击「定时刷新」，或用 MCP `set_scheduled_refresh` 设置。配置保存在 `config.yaml`，热生效、不重启内核。

- 使用**运行主机的系统时区**，时刻为 `HH:MM`，最多 24 个；面板显示该时区与下次执行时间，不随浏览器时区换算。
- 到点复用手动刷新的拉取、生成、校验和应用路径。全部来源失败、事务失败、初始化未完成或前台刷新占用时，按 1/5/15 分钟重试，最多 3 次且不越过下个计划时刻。
- 部分来源失败不重试：成功来源提交，失败来源沿用缓存。成功、无需执行或到达下个计划时刻时清零预算；用尽后等下个时刻。退避原因只写日志，可用 `journalctl -u miao` 查看；订阅行与告警同手动刷新。
- 调度仅在 Miao 进程存活时运行。内核停止时仍更新配置，但不会自动启核；调度退避不重置启动恢复预算。

### 禁用订阅节点

点击订阅卡片的节点数量可禁用/启用节点。禁用节点不进入配置，引用它的规则会阻断匹配流量；操作后节点池为空（含手动节点）返回 400。

禁用集存于 `volatile.yaml`，按“订阅 URL + 节点名”匹配；同名节点一起禁用，改名后旧条目失效。默认易变层随系统重启清空：

```yaml
disabled_nodes:
  - sub: "https://your-subscription-url"
    name: "香港 01"
```

## 连接检查

面板「连接检查」、REST `/api/connectivity` 和 MCP `test_connectivity` 共用一次 HTTP/HTTPS HEAD 请求，超时 5 秒，不修改配置。请求从 **Miao 主机**发起，忽略 HTTP_PROXY/HTTPS_PROXY，但仍可能经过系统 TUN；不代表浏览器或指定节点的连通性。

`success` 仅表示收到 HTTP 响应，须结合 `http_status`、`error_kind`（timeout/connect/request）和 `error` 判断。403/405/503 不代表访问正常，部分网站不支持 HEAD；后端不可达也不能当作目标不可达。关闭弹窗不取消检查，报告保留至下次检查或刷新页面；复制失败可手动选取。

## VPS 一键部署

「添加节点 → VPS 部署」通过 SSH 安装 Hysteria2 或恢复已有 Miao 节点。Miao 主机须有 OpenSSH 客户端。

- 目标为 Linux x86_64/arm64、systemd/OpenRC、POSIX sh；自动安装依赖支持 apt、dnf/yum、apk、pacman、zypper，其他环境须预装工具。
- 仅支持 **root 密码登录、SSH 22 端口**，不支持密钥或非 root sudo；密码只用于本次连接，不写入 Miao 配置。
- 固定版本经官方校验清单验证后才替换服务。Hysteria2 使用 **543/UDP**，须在安全组与 VPS 防火墙放行。

错误详情保留至关闭或重试。`Permission denied` 不能区分密码错误与服务端禁用登录；面板「查看解决办法与错误详情」提供[条件化排障步骤](../crates/miao-core/src/services/vps/ssh-auth-help.txt)，Miao 不自动执行修复。主机密钥变化时先核对服务商指纹，再更新运行 Miao 账户的 `known_hosts`。

## MCP：让 AI agent 操作代理

设置 `mcp: true` 或打开顶部 MCP 开关，端点为 `http://<面板地址>/mcp`；关闭时返回 404。Claude、Codex 图标可复制添加命令；「AGENTS.md」按钮复制端点、常用工具和确认约定，不自动配置客户端，agent 须能访问该地址。复制失败时弹窗保留文本。

使用 MCP `2025-11-25` Streamable HTTP：先 `initialize`，再 `notifications/initialized`，后续请求携带 `MCP-Protocol-Version`；无 session、无 SSE，启用时 `GET /mcp` 返回 405。用 `tools/list` 获取工具和参数，覆盖状态、连接、启停、节点、订阅、规则、定时刷新、VPS 与升级；平台不支持的操作返回明确错误。主题、弹窗、PWA、分享链接解析在浏览器本地完成，结构化节点用 `add_node` / `import_nodes` 导入。

停止、删除、部署、关闭 MCP、升级等工具要求 `confirm: true`，调用前须获用户明确确认。订阅 URL、连接记录和 VPS 密码应保密；热应用可能影响当前连接。MCP 与面板共用[无鉴权的访问边界](platforms.md#访问与持久化)。
