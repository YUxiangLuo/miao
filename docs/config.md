# 配置参考

启动时优先读取可执行文件旁的 `config.yaml`，否则使用[平台默认路径](profiles.md#文件位置)。文件不存在也能启动，可在面板向导中完成配置。面板与 MCP 无鉴权，访问边界见[平台说明](platforms.md#访问与持久化)。

## 命令行启动

```bash
sudo ./harbor --config /path/to/config.yaml
sudo ./harbor --sub 'https://example.com/sub?token=xxx' JP
./harbor --help
./harbor --version
```

- `--config=PATH` 与 `--sub=URL` 同样有效且互斥；参数在提权、写文件和启核前校验。Windows 桌面端额外支持 `--minimized`（静默启动到托盘），但仅接受 `--config` 而不支持 `--sub`。
- `--sub` 接收 HTTP/HTTPS Clash YAML 订阅，地区可选 `HK/JP/TW/SG/US`（不区分大小写；省略为手动，无候选时临时回退手动）。临时运行默认 `rule` 模式、不限倍率，不继承已有配置或偏好，正常退出自动清理，见 [Profile 生命周期](profiles.md#sdk-与生命周期)。
- 订阅 URL 须加引号（注意令牌会进入 shell 历史与进程参数）。单个订阅响应上限 **4 MiB**、原始 `proxies` 上限 **10000 项**（超限按解析失败处理，保留旧缓存）。
- **不支持多开**：TUN（`sing-tun`）与内部 Clash API（`:6262`）共享，启动前须先退出已有实例。

## 配置文件（`config.yaml`）

```yaml
port: 6161
subs:
  - "https://your-subscription-url"
nodes:  # sing-box outbound JSON（支持 8 种协议，详见内核能力）
  - '{"type":"hysteria2","tag":"香港跳板","server":"relay.example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
  - '{"type":"socks","tag":"美区静态住宅A","server":"198.51.100.10","server_port":1080,"version":"5","username":"user","password":"password","detour":"香港跳板"}'
custom_rules:
  - '{"domain_suffix":"example.com","action":"route","outbound":"direct"}'
  - '{"source_ip_cidr":"192.168.8.180/32","action":"route","outbound":"美区静态住宅A"}'
  - '{"process_name":"qbittorrent","action":"route","outbound":"香港跳板"}'  # Windows 进程名需带 .exe
terminals:  # 局域网设备档案（修改备注不触发内核重载）
  - ip: "192.168.8.180"
    name: "运营工位 01"
    department: "美区业务"
access_ports:  # 局域网独立 HTTP/SOCKS5 代理端口（mixed 入站）
  - id: "ap-20101"
    name: "美区一店"
    port: 20101
    outbound: "美区静态住宅A"
    username: "shop1"
    password: "secret"

# 以下均可选；node_select / max_multiplier / route_mode 为易变层缺失时的启动默认值
mcp: false
node_select: manual  # manual | fastest_hk | fastest_jp | fastest_tw | fastest_sg | fastest_us
max_multiplier: null  # 不限，或正十进制数（如 2.5）
route_mode: rule  # rule | global
scheduled_refresh:
  enabled: false
  times: ["04:30", "16:30"]  # 运行主机本地时区，最多 24 个
```

## 路由与 DNS 匹配顺序

节点支持范围见[内核能力](kernel.md#能力与裁剪)。默认 **`rule`** 模式下，流量按以下顺序匹配：

1. **协议嗅探与 DNS 劫持**（`sniff` → `hijack-dns`）
2. **私网直连**（`ip_is_private` → `direct`）
3. **独立代理端口**（`access_ports` 的 `access-port:<id>` 入站 → 绑定出口；出口不可用时原位 `reject`）
4. **中国大陆直连**（`chinasite` / `chinaip` → `direct`）
5. **自定义规则**（按添加顺序；可填 `proxy`、`direct`、`reject` 或节点 tag。引用缺失/禁用节点时原位改为 `reject`，保留原规则，节点恢复后自动恢复）
6. **订阅 URL 主机直连**（精确域名/IP → `direct`，避免失效默认代理阻断订阅刷新；可被前置自定义规则覆盖）
7. **默认代理**（`proxy`）

DNS 规则同步镜像上述策略：**独立代理端口 DNS（直连绑定出口或 `reject`）→ 大陆域名直连（`rule` 模式）→ 可镜像的自定义规则（每个指定节点使用独立 `custom-node-dns:<tag>` DoH 服务，避免节点变动或规则重排串用缓存）→ 订阅主机 `local` DNS → 默认远程 DoH**。

显式切换 **`global`** 模式仅移除内置私网与大陆直连（第 2、4 步及大陆直连 DNS），独立代理端口、自定义规则与订阅主机直连均保留。

> 离线 `reject` 熔断由运行中的内核执行；主动停服或删除最后一个可用来源导致停服后，规则无法拦截系统流量。

## 局域网设备与独立代理端口

在面板左侧 **`分流隔离`** 管理局域网整机与本地多端口两类隔离出口：

- **设备（按局域网 IP 固定整机出口）**：
  - 自动读取 `/tmp/dhcp.leases`（回退 `/var/lib/misc/dnsmasq.leases`）并与 `terminals` 档案合并；修改备注不重载内核，通过 `POST /api/terminals/assign` 单事务切换整机出口（实际生成/更新 `/32` `source_ip_cidr` 规则）。
  - 网络前提：路由器为终端分配静态 DHCP IPv4，到达 Harbor 前不可将多台设备 NAT 成同一来源 IP；避免独立 IPv6 出口绕过网关，且客户端 DNS 查询应直接经过 Harbor（而非先经路由器 DNS 转发器抹掉来源 IP）。
- **独立代理端口（`access_ports`，同机多开隔离）**：
  - 为每个条目创建 `mixed`（HTTP + SOCKS5 TCP/UDP）入站，监听端口范围 `1024–65535`（不可占用面板端口与内部 Clash 端口 `6262`），支持可选用户名/密码认证，上游凭据仅留存在网关内。
  - 优先级高于大陆分流与整机规则；绑定的出口或其 `detour` 前置节点不可用时，路由与 DNS 同步原位 `reject`。

## 手动节点、SOCKS5 与链式中转（`detour`）

面板 **`添加节点`** 对话框提供 **`粘贴链接`**、**`手动配置`**、**`VPS 部署`**（非 Windows）三种入口：

- **SOCKS5**：支持 `手动配置` 填写或 `粘贴链接` 导入 `socks5://user:pass@host:1080#名称`、`socks://...`、`host:1080:user:pass`（每行一条，特殊字符需 URL 编码，IPv6 加方括号）；用户名与密码须同填或同空（1–255 字节）。Clash YAML 订阅支持 `type: socks5`（不支持 SOCKS5 over TLS；`udp: false` 仅启 TCP）。底层生成 sing-box `type: socks, version: "5"`。
- **前置跳板（`detour`）**：手动节点 JSON 与 `手动配置` 面板均支持指定前置节点 tag；禁止填写 `proxy`/`direct`/`reject`、禁止自引用或成环。当前置节点缺失或被禁用时，仅排除受影响节点及其依赖链（不阻塞其他可用节点），同时熔断绑定到它们的规则与端口；原始配置保留，前置恢复后自动恢复。
- **测速超时**：面板与 MCP 手动测速给予 8 秒探测、10 秒 HTTP 上限，兼容链式中转额外握手耗时。

## 节点选择与倍率过滤

稳定配置、易变状态与偏好分文件保存，见[文件位置](profiles.md#文件位置)。

- **策略**：`manual` 从全量节点池手动选择（由 `.last_proxy` 恢复）；`fastest_{hk,jp,tw,sg,us}` 使用 `urltest` 自动测速优选，地区无候选时临时回退 `manual` 但不覆盖用户的 requested 策略。
- **最高倍率**：从当前显示名识别（如 `18x`、`6.5X`、`2.4倍`、`倍率：1.3`，未标按 `1x`），**仅筛选 `fastest_*` 候选**，`manual` 保留完整池；“不限”在偏好文件中记为 `unlimited`。

## 订阅管理

- **启动与后台恢复**：存在兼容缓存、订阅快照或手动节点时先本地启核再后台刷新，失败按指数退避自动重试并最终转 30 分钟低频重试；无可用节点时转 30 分钟等待更新。详细状态语义见[订阅刷新与恢复](runtime-state.md#订阅刷新与恢复)。
- **禁用节点**：在 **`代理节点 → 节点源管理`** 打开订阅详情即可禁用/启用节点。禁用集存于 `volatile.yaml`（按「订阅 URL + 节点名」匹配，同名连坐；Linux/OpenWrt 重启清空）；不允许禁用后导致全部可用节点为空。
- **定时刷新**：在 **`代理节点 → 节点源管理`** 或 **`系统设置 → 订阅定时刷新`**（或 MCP `set_scheduled_refresh`）配置。按**运行主机本地时区**解析 `HH:MM`（最多 24 个），热生效不重启内核；仅在 Harbor 进程存活时触发，内核停止时仅更新配置不自动启核。全部失败或忙碌时按 1/5/15 分钟重试（最多 3 次且不跨下次计划）。
- **删除订阅**：优先从剩余订阅的已接受快照本地筛选，不强制联网；无可用本地材料时才拉取剩余订阅。若确认剩余来源无可用节点，会清理旧运行配置并停服（保留快照以便重新启用禁用节点）；全部拉取失败且无可用缓存/手动节点时保留旧配置并报错。

## 连接检查

在 **`实时连接`**、REST `POST /api/connectivity` 与 MCP `test_connectivity` 均可触发：从 **Harbor 主机**发起单次 HTTP/HTTPS `HEAD` 请求（5 秒超时，忽略 `HTTP_PROXY`/`HTTPS_PROXY`，仍可能经系统 TUN），不修改配置。`success` 仅表示收到响应，需结合 `http_status` 与 `error_kind`（`timeout` / `connect` / `request`）判断。

## VPS 一键部署（仅非 Windows）

Linux/OpenWrt 在 **`添加节点 → VPS 部署`** 通过系统 OpenSSH 客户端部署 Hysteria2 或复用已有节点（Windows 不编译此路由，返回 404）：

- 目标要求：Linux `x86_64`/`arm64`、systemd 或 OpenRC、POSIX sh；自动依赖安装支持 `apt`/`dnf`/`yum`/`apk`/`pacman`/`zypper`。
- 认证方式：仅支持 **root 密码 + SSH 22 端口**，密码仅用于本次连接、不落盘；Hysteria2 监听 **543/UDP**（需在防火墙/安全组放行），二进制按官方校验清单验签后安装。
- 认证失败可在弹窗查看[排障步骤](../crates/harbor-core/src/services/vps/ssh-auth-help.txt)；主机密钥变更请核对服务商指纹后更新运行账户的 `known_hosts`。

## MCP（AI Agent 接入）

在 **`系统设置 → Model Context Protocol (MCP)`** 开启开关或配置 `mcp: true` 后启用 `http://<面板地址>/mcp`（关闭返回 404，开启时 `GET /mcp` 返回 405）。面板提供 Claude Code / Codex 添加命令及 `AGENTS.md` 片段一键复制（不自动改客户端配置）。

- 协议：MCP `2025-11-25` Streamable HTTP（无 session、无 SSE）；客户端需先 `initialize` → `notifications/initialized`，后续请求带 `MCP-Protocol-Version`。
- 工具集：通过 `tools/list` 获取（共 35 个工具），覆盖状态、连接/流量、启停、订阅、节点、规则、定时刷新、连通性检查、VPS 部署与升级；平台不支持的操作返回明确错误。
- 安全边界：停服、删除、部署、关闭 MCP、升级等破坏性工具要求 `confirm: true`，调用前须经用户明确确认；订阅 URL、连接记录与 VPS 密码应保密。
