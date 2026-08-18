<div align="center">
  <img src="frontend/public/icon.svg" width="72" alt="Miao logo" />
  <h1>Miao</h1>
  <p><strong>开箱即用的透明代理</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
    <a href="https://github.com/YUxiangLuo/miao/actions/workflows/ci.yml"><img src="https://github.com/YUxiangLuo/miao/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <img src="https://img.shields.io/badge/platform-Linux%20%7C%20OpenWrt%20%7C%20Windows-blue?style=flat-square" alt="Platform" />
  </p>
</div>

Miao 把 sing-box 内核、geo 分流规则和 Web 控制面板编进同一个可执行文件。在 Linux / OpenWrt 上它是 `sudo` 即跑的单二进制；在 Windows 上它是带系统托盘的桌面程序。没有配置文件也能启动——面板先给引导页，不落盘任何东西。

本文档是使用手册与配置参考。想看产品介绍与截图导览，去官网 <https://miao.vesein.dev>。

![Miao 控制面板](docs/screenshot.png)

## 安装

### Linux / OpenWrt

```bash
mkdir -p ~/miao && cd ~/miao

# amd64（arm64 把文件名换成 miao-rust-linux-arm64）
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao

chmod +x miao && sudo ./miao
```

面板在 <http://localhost:6161>，找不到配置时先进引导页。需要 root（创建 TUN 网卡、接管路由所需）；OpenWrt 启动时自动检测并安装内核依赖。运行时文件全部在 `/tmp/miao-sing-box`，系统重启即消失。

常驻为 systemd 服务（离线安装，重复运行即升级）：

```bash
sudo bash install.sh ./miao     # 二进制 → /usr/local/bin/miao，配置 → /etc/miao

systemctl status miao           # 状态
journalctl -u miao -f           # 日志
sudo bash remove.sh             # 卸载并清理全部痕迹（-y 跳过确认）
```

### Windows 桌面版

Win10/11 x64，从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 下载 `miao-windows-amd64-setup.exe`：

1. 安装到当前用户，安装本身不需要管理员（缺 WebView2 时安装包会引导下载）
2. 每次启动点一次 UAC（TUN/Wintun 需要管理员）；托盘菜单勾选「开机自启」可免 UAC 自启（任务计划实现，登录后直进托盘）
3. 关窗口进托盘，单击唤出、双击唤出/收回，托盘「退出」才停内核
4. 更新 = 先退出，再装新安装包（面板内无 Windows 一键升级）
5. 日志在 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`（超 8 MB 自动轮转），托盘「打开日志」直达

## 面板能力

| 模块 | 说明 |
| --- | --- |
| 服务状态 | 实时上下行速率、PID、运行时长；生命周期归托盘/systemd 管 |
| 代理模式 | 规则分流（国内直连/国外代理）⇄ 全局代理，一键切换 |
| 节点列表 | 订阅 + 手动节点平铺为一个池；单个/批量测速，点击切换，重启后恢复上次选择 |
| 链接统计 | 活动连接按站点聚合：实时速度、累计流量，展开看逐条明细 |
| 自定义规则 | 域名/IP/端口/进程名/进程路径 → 直连/代理/拦截/**指定节点**；节点失效自动跳过并在列表标记 |
| 手动节点 | 分享链接批量导入（`hysteria2://` `hy2://` `ss://` `vmess://` `vless://` `trojan://` `tuic://` `anytls://`）或手动填写；VPS 一键部署 Hysteria2（仅 Linux） |
| 订阅管理 | Clash YAML 订阅添加/刷新，失败自动回退缓存配置 |
| 版本升级 | 仅 Linux：检测新版本并面板内一键自升级（SHA256 校验） |
| MCP | 面板右下角一键开启 MCP 端点并复制地址，AI agent 即可操作代理 |

## 配置参考

不创建任何文件也能用。查找顺序：`--config` → 可执行文件同目录 `config.yaml` → 平台默认路径。

```yaml
port: 6161                 # 面板端口

subs:                      # Clash YAML 订阅
  - "https://your-subscription-url"

nodes:                     # 手动节点（sing-box outbound JSON）
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'

custom_rules:              # 可选：优先于内置分流，全局模式下仍生效
  - '{"domain_suffix":"example.com","action":"route","outbound":"direct"}'
  # 进程级指定出口：让 qbittorrent 的流量固定走「香港节点」（Windows 写 qbittorrent.exe）
  - '{"process_name":"qbittorrent","action":"route","outbound":"香港节点"}'
  # outbound 除 proxy/direct 外也可填节点 tag；节点消失时该规则被跳过并在面板标记

mcp: true                  # 可选：MCP 端点（POST /mcp），默认关闭

route_mode: global         # 可选：启动默认路由模式（rule 规则分流 / global 全局代理）
```

**数据按三层落盘**：

| 层 | 文件 | 内容 | 位置 |
| --- | --- | --- | --- |
| 稳定层 | `config.yaml` | 订阅/节点/规则等低频配置 | Linux/OpenWrt：`/etc/miao`；Windows：应用数据目录 |
| 易变层 | `volatile.yaml` | 节点选择策略、路由模式 | Unix：`/tmp/miao-sing-box`（tmpfs，系统重启后回到 config.yaml 的启动默认值）；Windows：应用数据目录（持久） |
| 状态层 | `config.json` / `.cache` / 快照 | 运行时配置与缓存 | sing-box 运行目录，可删 |

OpenWrt 的易变层写 tmpfs：切节点/切模式这类高频操作零闪存磨损。面板/进程重启（如自升级）两层都保留，选择与模式不丢失。

## MCP：让 AI agent 操作代理

配置里加一行 `mcp: true`（默认关闭），端点是 `POST http://<面板地址>/mcp`（MCP 2026-07-28，无状态 JSON-RPC，无握手无会话）。面板右下角的浮动控件可以一键开关并复制地址。

内置工具：`get_status`（服务状态）、`list_nodes`（平铺节点池）、`switch_node`（切节点，持久化）、`set_node_select`（手动⇄地区最快）、`test_delay`（测速）、`set_route_mode`（分流⇄全局）、`refresh_subscriptions`（刷新订阅）、`list_rules`、`list_connections`。

连接时服务端通过 `instructions` 告知调用者：「你的流量很可能正经过本代理，破坏性操作会自断其网」——agent 在执行热重启类操作前会先找你确认。

> **安全提示**：Linux 下面板绑 `0.0.0.0` 且无鉴权，开启 MCP 后局域网内任何设备都能调用这些工具（包括切节点/切模式），请自行评估网络环境。Windows 版只听 `127.0.0.1`，无此问题。

## 工作原理

```
┌─────────────┐   TUN    ┌──────────────────┐   Clash API   ┌─────────┐
│  本机全部流量  │ ───────▶ │ 内嵌 sing-box 内核 │ ◀──────────▶ │ Web 面板 │
└─────────────┘          │ geoip-cn + 直连域名 │  127.0.0.1   │ (内嵌)   │
                         │ 规则集决定分流去向   │              └─────────┘
                         └──────────────────┘
```

- 透明代理由 sing-box 的 TUN inbound 完成：Linux 用 `auto_route` + `auto_redirect`（nftables），Windows 用 `auto_route` + `strict_route`（Wintun 已编进内核），都不手碰防火墙
- DNS 双轨：国外域名经代理走 Cloudflare DoH，国内直连 223.5.5.5；缓存落在运行时 `cache.db`
- **启动秒开**：上次成功运行的配置缓存直接起内核，订阅在后台刷新——有变化才重启内核，全部失败则继续用缓存运行并告警
- 内核异常退出自动拉起（退避重试，连续失败后面板告警）
- 面板与内核之间只走 Clash API（`127.0.0.1:6262`）；配置变更先 `sing-box check` 校验再热重启，失败回滚
- 内核与规则集每次启动重新释放，保证与当前二进制一致；`cache.db` / 配置缓存有意保留

## 平台对照

| | Linux / OpenWrt | Windows |
| --- | --- | --- |
| 分发 | 单个 musl 二进制 | NSIS 安装包（需 WebView2） |
| 提权 | `sudo` | 每次启动一次 UAC |
| 面板 | 浏览器打开 `localhost:6161`，默认听 `0.0.0.0` | 自带窗口，听 `127.0.0.1` |
| 配置 | `/etc/miao/config.yaml` | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\config.yaml` |
| 内核运行时 | `/tmp/miao-sing-box` | `%TEMP%\miao-sing-box` |
| 易变配置 | `/tmp/miao-sing-box/volatile.yaml`（tmpfs） | 应用数据目录（持久） |
| 一键升级 / VPS 部署 | 有 | 不编进桌面进程 |
| 开机自启 | `install.sh` → systemd | 托盘勾选（任务计划，登录免 UAC 直进托盘） |

## 从源码构建

依赖：Bun、Go、Rust、curl。克隆后一条命令（只构建本机架构，跨架构由 CI 负责）：

```bash
./build.sh        # 产物: target/release/miao-rust
```

构建脚本依次完成前端打包、sing-box 源码编译与 geo 规则集下载；内核默认跟 sing-box 仓库默认分支，`SING_BOX_REF` 可钉版（分支/tag/commit sha）。编 Windows 内核：

```bash
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh
```

桌面壳在 workspace 里但不是 default member（Linux `cargo test` 不会去链 WebView）：

```bash
cargo build -p miao-desktop            # Windows 原生可编；Linux 上编要 webkit2gtk
cargo check -p miao-core --target x86_64-pc-windows-gnu   # Linux 上的 Windows 静态门禁
```

开发：

```bash
bun run --cwd frontend dev      # 前端开发服务器，API 代理到 localhost:6161
bun run --cwd frontend test     # 前端测试
cargo test                      # 后端测试（default members，不含桌面壳）
```

注意：改前端后必须 `./scripts/build-frontend.sh` 再编 Rust——`include_str!` 嵌的是 `public/index.html`，只 `cargo build` 还是旧页面。更多开发约定见 [DEV_NOTES.md](DEV_NOTES.md)。

## 常见问题

**为什么要 root / 管理员？**
TUN 透明代理要创建虚拟网卡并接管整机路由，这是内核级权限，除此之外没有别的依赖。

**配置和状态分别存在哪？**
见「配置参考」的三层落盘表。你手写的只有 `config.yaml`；模式与节点选择在 `volatile.yaml`；其余都是运行时文件，删掉不影响下次启动。

**Linux 下系统重启后，路由模式/地区自动选择回到了默认值？**
预期行为：易变层在 tmpfs 上，系统重启后回到 `config.yaml` 的启动默认值（fail-safe）。进程重启（systemd restart、面板自升级）不丢；手动选择的具体节点由 `.last_proxy` 持久恢复。

**能把面板安装成桌面应用吗？**
可以，面板是 PWA。Chrome/Edge 打开 `localhost:6161` 后地址栏右侧会出现「安装」图标（或菜单 → 安装 Miao），装完有独立窗口和启动器图标，没有浏览器边框。安装入口只在本机 `localhost` 下可用——局域网 IP 访问不是安全上下文，浏览器不允许安装。不想安装的话浏览器书签照旧用。

**面板有鉴权吗？**
没有。Linux 监听 `0.0.0.0:6161`，请在可信局域网使用、勿暴露公网，需要暴露时套一层带鉴权的反向代理。Windows 只听 `127.0.0.1`。

**怎么干净卸载？**
`sudo bash remove.sh`：服务、二进制、`/etc/miao`、`/tmp/miao-sing-box`、残留 sing-box 进程与 `sing-tun` 网卡全部清理。Windows 从系统设置卸载，卸载前先从托盘退出。

## 技术栈

Rust（axum）控制面 · 内嵌 sing-box 内核 · React + Vite 面板（打成单文件 HTML 嵌进二进制）· Windows 上 Tauri 2 桌面壳 · MCP 无状态 JSON-RPC 端点 · GitHub Actions 出 Linux musl 与 Windows 桌面版

[MIT License](LICENSE)
