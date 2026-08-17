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

搭一个分流代理，通常意味着：装内核、写配置、调防火墙、再找个面板。Miao 把这些打包成**一个文件**——内嵌 sing-box 内核、分流规则和 Web 面板：Linux / OpenWrt 下载、`sudo` 运行、浏览器自动打开；Windows 是同一块面板外套一层 Tauri 窗口，一次 UAC 代替 `sudo`。没有配置文件也能跑，进去就是引导页。

![screenshot](docs/screenshot.png)

## 30 秒上手

### Linux / OpenWrt

```bash
mkdir -p ~/miao && cd ~/miao

# amd64（arm64 把文件名换成 miao-rust-linux-arm64）
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao

chmod +x miao && sudo ./miao
```

浏览器打开 <http://localhost:6161>，按引导页添加订阅或节点即可。需要 root（TUN 所需）；找不到配置时先进引导页，不落盘任何文件。

装成开机自启的 systemd 服务（离线，重复运行即升级）：

```bash
sudo bash install.sh ./miao     # 装到 /usr/local/bin/miao，配置在 /etc/miao

systemctl status miao           # 状态
journalctl -u miao -f           # 日志
sudo bash remove.sh             # 卸载（-y 跳过确认）
```

### Windows 桌面版

Win10/11 x64。透明代理走 TUN（`auto_route` + `strict_route`，Wintun 已编进内核），已在真机跑通。从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 下载 `miao-windows-amd64-setup.exe`：

1. 安装到当前用户，**安装本身不要管理员**（系统通常已有 WebView2，没有的话安装包自动引导下载）
2. 每次启动点一次 UAC（TUN/Wintun 要管理员）；想要免 UAC 开机自启：托盘菜单勾选「开机自启」（任务计划实现，登录后直接进托盘）
3. 窗口打开即铺满工作区；关窗口进托盘，单击托盘唤出、双击唤出/收回，托盘「退出」才停内核
4. 更新：先退出，再装新安装包（安装器检测到运行中的 miao 会提示你先退出——它杀不掉提权进程，不退出就装会留下旧文件）。面板内没有 Windows 一键升级
5. 出问题：托盘「打开日志」，日志在 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`（超 8 MB 自动轮转）

## 面板里有什么

| 模块 | 能做什么 |
| --- | --- |
| 服务状态 | 实时上下行速率、PID 与运行时长（生命周期归托盘/systemd 管，面板不持有启停按钮） |
| 代理模式 | 规则分流（国内直连 / 国外代理）⇄ 全局代理，一键切换 |
| 节点列表 | 网格化平铺全部节点（订阅 + 手动），单个/批量延迟测试，点击即切换，重启后自动恢复上次选择 |
| 链接统计 | 活动连接按站点聚合：实时速度、累计流量，展开看每条连接明细 |
| 自定义规则 | 域名/IP/端口/进程名/进程路径等条件，目标可直连/代理/拦截/**指定节点**；节点失效自动跳过并在列表标记提醒 |
| 去广告 | 内嵌广告规则集，路由层拦截，一键开关 |
| 手动节点 | 粘贴分享链接批量导入（`hysteria2://` `hy2://` `ss://` `vmess://` `vless://` `trojan://` `tuic://` `anytls://`），或手动填写；**VPS 一键部署**（仅 Linux）：填 VPS 的 IP 和 root 密码，自动部署 Hysteria2 并回写节点，密码不保存 |
| 订阅管理 | 添加/刷新 Clash YAML 订阅，失败自动回退缓存配置 |
| 版本升级 | 仅 Linux：检测新版本并面板内一键自升级 |
| MCP 开关 | 首页右下角：一键开启 MCP 端点并复制地址，AI agent 即可操作代理 |

## MCP：让 AI agent 操作你的代理

配置里加一行开启：

```yaml
mcp: true                  # 默认关闭
```

端点是 `POST http://<面板地址>/mcp`（MCP 2026-07-28，无状态 JSON-RPC，无握手无会话）。面板右下角的浮动控件可以一键开关并复制地址。

内置工具：`get_status`（服务状态）、`list_nodes`（平铺节点池）、`switch_node`（切节点，持久化）、`test_delay`（测速）、`set_route_mode`（分流⇄全局）、`list_rules`、`list_connections`。与面板同一套心智模型：没有分组概念，所有订阅和手动节点组成一个节点池。

连接时服务端会通过 `instructions` 告知调用者：「你的流量很可能正经过本代理，破坏性操作会自断其网」——agent 在执行热重启类操作前会先找你确认。

> 安全提示：Linux 下面板绑 `0.0.0.0`，开启 MCP 后局域网内任何设备都能调用这些工具（包括切换节点/切模式），请自行评估网络环境。Windows 版只听 `127.0.0.1`，无此问题。

## 它是怎么工作的

```
┌─────────────┐   TUN    ┌──────────────────┐   Clash API   ┌─────────┐
│  本机全部流量  │ ───────▶ │ 内嵌 sing-box 内核 │ ◀──────────▶ │ Web 面板 │
└─────────────┘          │ geoip-cn + 直连域名 │  127.0.0.1   │ (内嵌)   │
                         │ 规则集决定分流去向   │              └─────────┘
                         └──────────────────┘
```

- 透明代理由 sing-box 的 TUN inbound 完成：Linux 用 `auto_route` + `auto_redirect`（nftables），Windows 用 `auto_route` + `strict_route`，都不手碰防火墙
- DNS 双轨：国外域名经代理走 Cloudflare DoH，国内直连 223.5.5.5；缓存落在运行时 `cache.db`
- **启动秒开**：优先用上次成功运行的配置缓存直接起内核，订阅在后台刷新——有变化才重启内核，全部失败则继续用缓存运行并告警
- 内核异常退出会自动拉起（退避重试，连续失败后面板告警）
- 面板通过 Clash API（`127.0.0.1:6262`）切节点、读连接；配置变更先 `sing-box check` 校验再热重启，失败回滚
- 内核与规则集每次启动重新释放，保证与当前二进制一致；`cache.db` / 配置缓存有意保留

## 两个平台的对照

| | Linux / OpenWrt | Windows |
| --- | --- | --- |
| 拿到手 | 一个 musl 文件 | NSIS 安装包（要 WebView2） |
| 提权 | `sudo` | 每次运行 UAC |
| 面板 | 浏览器打开 `localhost:6161`，默认听 `0.0.0.0` | 自带窗口，听 `127.0.0.1` |
| 配置 | `/etc/miao/config.yaml` | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\config.yaml` |
| 运行时内核 | `/tmp/miao-sing-box` | `%TEMP%\miao-sing-box` |
| 易变配置 | `/tmp/miao-sing-box/volatile.yaml`（tmpfs） | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\volatile.yaml`（持久） |
| 一键升级 / VPS 部署 | 有 | 不编进桌面进程 |
| 开机自启 | `install.sh` → systemd | 托盘勾选（任务计划，登录免 UAC 直进托盘） |

## 配置参考（进阶）

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

adblock: true              # 可选：去广告（路由层拦截，默认关闭）

mcp: true                  # 可选：MCP 端点（POST /mcp），默认关闭

route_mode: global         # 可选：启动默认路由模式（rule 规则分流 / global 全局代理）
```

> **数据分三层落盘**：`config.yaml` 是稳定层（订阅/节点/规则等低频配置，Linux/OpenWrt 在 `/etc/miao`）；节点选择策略与路由模式是易变层，写在 `volatile.yaml`——OpenWrt/Linux 落在 tmpfs（`/tmp/miao-sing-box`），避免切节点/切模式这类高频写入磨损路由器闪存，系统重启后回到 `config.yaml` 的启动默认值；Windows 落在应用数据目录，持久保存。面板/进程重启（如自升级）两层都保留，选择与模式不丢失。

## 从源码构建

依赖：Bun、Go、Rust、curl。克隆后一条命令（只构建本机架构，跨架构由 CI 负责）：

```bash
./build.sh        # 产物: target/release/miao-rust
```

构建脚本依次完成前端打包、sing-box 源码编译与 geo 规则集下载；内核默认拉 sing-box 仓库默认分支，可用 `SING_BOX_REF` 覆盖（分支/tag/完整 commit sha 均可）。

编 Windows 内核（Go 跨平台编译，Linux/Windows 开发机均可）：

```bash
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh
```

桌面壳在 workspace 里，但不是 default member（Linux `cargo test` 不会去链 WebView）：

```bash
cargo build -p miao-desktop            # Windows 原生可编（VS C++ 工具链）；Linux 上编要 webkit2gtk
cargo check -p miao-core --target x86_64-pc-windows-gnu   # Linux 上的 Windows 静态门禁
```

NSIS 安装包由 CI 的 `windows-latest` job 出；Windows 开发机装了 Node + Tauri CLI 后也可本地 `tauri build --bundles nsis`。

开发前端：

```bash
bun run --cwd frontend dev      # 开发服务器，API 代理到 localhost:6161
bun run --cwd frontend test     # 前端测试
cargo test                      # 后端测试
```

改前端后必须 `./scripts/build-frontend.sh` 再编 Rust：`include_str!` 嵌的是 `public/index.html`，只 `cargo build` 还是旧页面。

## 技术栈

Rust（axum）控制面 · 内嵌 sing-box 内核 · React + Vite 面板（打成单文件 HTML 嵌进二进制）· Windows 上再用 Tauri 2 当窗口 · MCP 无状态 JSON-RPC 端点 · GitHub Actions 单分支出 Linux musl 与 Windows 桌面版
