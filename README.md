<div align="center">
  <img src="frontend/public/icon.svg" width="72" alt="Miao logo" />
  <h1>Miao</h1>
  <p><strong>一个文件，让整台设备开箱即用透明代理。</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
    <a href="https://github.com/YUxiangLuo/miao/actions/workflows/ci.yml"><img src="https://github.com/YUxiangLuo/miao/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <img src="https://img.shields.io/badge/platform-Linux%20%7C%20OpenWrt-blue?style=flat-square" alt="Platform" />
  </p>
</div>

在 Linux 或路由器上搭一个分流代理，通常意味着：装内核、写配置、调防火墙、再找个面板。Miao 把这一切打包成**单个可执行文件**——内嵌 sing-box 内核、分流规则与 Web 面板，下载、`sudo` 运行、浏览器自动打开，结束。

![screenshot](docs/screenshot.png)

## 30 秒上手

```bash
mkdir -p ~/miao && cd ~/miao

# amd64(arm64 把文件名换成 miao-rust-linux-arm64)
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao

chmod +x miao && sudo ./miao
```

浏览器打开 <http://localhost:6161>，按引导页添加订阅或节点即可。需要 root(TUN 透明代理所需）；找不到配置时会先进入引导页，不落盘任何文件。

### 用脚本安装为 systemd 服务

如果手头已有下载好的二进制，可以用仓库里的脚本一键安装为开机自启的 systemd 服务（全程离线）:

```bash
sudo bash install.sh ./miao
```

脚本会安装到 `/usr/local/bin/miao`，配置与状态文件位于 `/etc/miao`，注册并启动 `miao.service`。重复运行即升级。

管理服务：

```bash
systemctl status miao    # 查看状态
journalctl -u miao -f    # 查看日志
```

### 卸载清理

```bash
sudo bash remove.sh        # 交互确认;-y 跳过确认
```

会停止并删除 systemd 服务、二进制、`/etc/miao` 配置目录与 `/tmp/miao-sing-box` 运行时文件，并清理可能残留的 sing-box 进程与 `sing-tun` 网卡。

## 面板里有什么

| 模块 | 能做什么 |
| --- | --- |
| 服务状态 | 启停 sing-box、实时上下行速率、PID 与运行时长 |
| 代理模式 | 规则分流（国内直连 / 国外代理）⇄ 全局代理，一键切换 |
| 节点选择 | 网格化节点列表，单个/批量延迟测试，点击即切换，重启后自动恢复上次选择 |
| 链接统计 | 活动连接按站点聚合：实时速度、累计流量，展开可看每条连接明细 |
| 自定义规则 | 域名/IP/端口/进程名/进程路径等条件，目标可直连/代理/拦截/**指定节点**；节点失效自动跳过并在列表标记提醒 |
| 去广告 | 内嵌广告规则集，连接层拦截，一键开关 |
| 手动节点 | 粘贴分享链接批量导入，或手动填写（高级参数折叠收纳） |
| 订阅管理 | 添加/刷新 Clash YAML 订阅，失败自动回退缓存配置 |
| 版本升级 | 检测新版本并面板内一键自升级 |

## 节点的三种来法

1. **订阅**——粘贴 Clash YAML 格式订阅链接
2. **粘贴分享链接**——`hysteria2://` `hy2://` `ss://` `vmess://` `vless://` `trojan://` `tuic://` `anytls://`，每行一条，实时预览、批量导入
3. **VPS 一键部署**——「添加节点 → VPS 部署」里填入 VPS 的 IP 和 root 密码，自动在远端部署 Hysteria2 并把节点写回本地；密码仅用于本次部署，不会被保存

## 它是怎么工作的

```
┌─────────────┐   TUN    ┌──────────────────┐   Clash API   ┌─────────┐
│  本机全部流量  │ ───────▶ │ 内嵌 sing-box 内核 │ ◀──────────▶ │ Web 面板 │
└─────────────┘          │ geoip-cn + 直连域名 │  127.0.0.1   │ (内嵌)   │
                         │ 规则集决定分流去向   │              └─────────┘
                         └──────────────────┘
```

- 透明代理由 sing-box 的 TUN inbound 完成（`auto_route` + `auto_redirect`)，不碰 iptables
- DNS 双轨：国外域名经代理走 Cloudflare DoH（https://1.1.1.1/dns-query），国内直连（223.5.5.5）；缓存落在运行时 `cache.db`，TTL 过期后 8 小时内先返回旧值再后台刷新
- 面板通过 Clash API 切换节点、读取连接统计；配置变更先 `sing-box check` 校验再热重启
- 运行时文件在 `/tmp/miao-sing-box`；内核与规则集每次启动重新释放，保证与当前二进制一致

## 配置参考（进阶）

不创建任何文件也能用；需要持久化或精细控制时，按此顺序查找： `--config 指定路径` → 可执行文件同目录 `config.yaml` → `/etc/miao/config.yaml`。

```yaml
port: 6161                 # 面板端口

subs:                      # Clash YAML 订阅
  - "https://your-subscription-url"

nodes:                     # 手动节点(sing-box outbound JSON)
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'

custom_rules:              # 可选:sing-box 路由规则,优先于内置分流,全局代理模式下仍生效
  - '{"domain_suffix":"example.com","action":"route","outbound":"direct"}'
  # 进程级指定出口:让 qbittorrent 的流量固定走「香港节点」
  - '{"process_name":"qbittorrent","action":"route","outbound":"香港节点"}'
  # outbound 除 proxy/direct 外也可填节点 tag(面板「添加规则」可直接选进程名/路径 + 指定节点);
  # 节点日后消失时该规则会在生成配置时被跳过,不阻塞服务,面板规则列表会有失效标记与 toast 提醒

adblock: true              # 可选:去广告(内嵌规则集,连接层拦截,默认关闭)
```

> 注意：面板里的分流/全局模式是会话级状态，重启后总是回到规则分流，不会写入配置文件。

## 从源码构建

依赖：Bun、Go、Rust、curl。克隆后一条命令（只构建本机架构，跨架构由 CI 负责）:

```bash
./build.sh        # 产物: target/release/miao-rust
```

构建脚本会依次完成前端打包、sing-box 源码编译与 geo 规则集下载；可用 `SING_BOX_REF` 指定 sing-box 的分支或 tag。

开发前端：

```bash
bun run --cwd frontend dev      # 开发服务器,API 代理到 localhost:6161
bun run --cwd frontend test     # 前端测试
cargo test                      # 后端测试
```

## 技术栈

Rust(axum)后端 · sing-box 内核 · React + Vite 前端（构建为单文件 HTML 内嵌进二进制）· GitHub Actions 跨架构发布
