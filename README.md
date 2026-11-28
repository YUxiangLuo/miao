<div align="center">
  <img src="frontend-rsbuild/public/icon.svg" width="72" alt="Harbor logo" />
  <h1>Harbor</h1>
  <p><strong>单文件透明代理网关与多设备 / 多端口静态 IP 隔离面板</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/actions/workflows/ci.yml"><img src="https://github.com/YUxiangLuo/miao/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
  </p>
</div>

Harbor 将定制 `sing-box` 内核、大陆/内网直连规则集与 SolidJS 2.0 Web 控制台打包为单个可执行文件，支持 Linux / OpenWrt（`amd64`、`arm64`）与 Windows 桌面版（`x64`）：

- **双粒度隔离与离线熔断**：既可按局域网 IP 分配整机指定出口（读取 DHCP 租约自动发现设备，单事务无漏流切换），也可开放多组独立 HTTP/SOCKS5 代理端口（`access_ports`）供同机多开浏览器或客户端隔离使用。绑定节点或其前置跳板不可用时，路由与 DNS 双层原位 `reject` 阻断，不回退公共代理池，国内与内网直连不受影响。
- **多协议节点与链式中转（`detour`）**：支持 SOCKS5、Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC 八种协议与静态住宅前置跳板中转，内置订阅定时刷新、常用服务分流预设与实时连接监控。
- **单文件部署与 AI 运维**：零外部运行依赖，开箱即用；提供原生 MCP（Streamable HTTP）端点，便于 AI Agent 查询状态、测速与管理配置。

## 安装与启动

从 [Release](https://github.com/YUxiangLuo/miao/releases/latest) 下载对应平台的产物及同名 `.sha256` 校验文件：

| 平台与架构 | 下载文件 |
| --- | --- |
| Linux / OpenWrt (`x86_64`) | `harbor-linux-amd64` |
| Linux / OpenWrt (`aarch64`) | `harbor-linux-arm64` |
| Windows 桌面版 (`x64`) | `harbor-windows-amd64-setup.exe` |

### Linux / OpenWrt

先按[下载与校验](docs/platforms.md#下载与校验)核对 SHA-256。启动需要 root 权限，TUN 和 Clash API 不支持多开（OpenWrt 以 root 直接运行，省略 `sudo`）：

```bash
chmod +x ./harbor-linux-amd64 &&
sudo ./harbor-linux-amd64
```

浏览器访问 `http://<设备IP>:6161`（本机为 `http://localhost:6161`）完成向导配置。**面板和 MCP 无鉴权，Linux/OpenWrt 默认监听 `0.0.0.0`，只应允许可信网络访问。**

### Windows 桌面版

直接运行 `harbor-windows-amd64-setup.exe` 安装到当前用户目录（无需管理员；启动时经 UAC 提权并驻留托盘，仅监听 `127.0.0.1`）。覆盖升级或卸载前请先从托盘右键选择「退出」，详见[平台说明](docs/platforms.md#windows-桌面版)。

临时订阅与启动参数见[配置参考](docs/config.md#命令行启动)；systemd 服务安装、升级与卸载见[平台说明](docs/platforms.md)。

## 从源码构建

需要 Rust、Bun；首次准备内核与规则还需 Go、Git、curl。

```bash
./dev.sh build                                 # 构建前端与本机 release（产物 target/release/harbor-cli）
sudo bash install.sh ./target/release/harbor-cli  # 安装或更新 Linux systemd 服务（期间短暂断网）
```

Windows 本地打包桌面安装包（PowerShell / CMD 执行 `.\desktop\package.cmd`，Git Bash 执行 `./dev.sh build desktop`），交叉编译、测试与发布流程见[开发指南](docs/development.md)。

## 文档

| 类别 | 主题 | 入口 |
| --- | --- | --- |
| 使用与部署 | 命令行、配置文件、分流、设备/端口隔离、订阅与 MCP | [配置参考](docs/config.md) |
| 使用与部署 | 平台差异、下载校验、systemd / OpenWrt / Windows / PWA | [平台说明](docs/platforms.md) |
| 开发与维护 | 构建、检查、CI/Release、本机生产实例与 Windows 打包 | [开发指南](docs/development.md) |
| 开发与维护 | 前端架构与约束、定制 `sing-box` 内核维护 | [前端开发](frontend-rsbuild/README.md)、[内核维护](docs/kernel.md) |
| 开发与维护 | 运行状态机与配置事务、Profile 路径归属 | [状态与事务](docs/runtime-state.md)、[Profile 路径](docs/profiles.md) |

[Harbor 源码许可](LICENSE) · [内核来源与许可](docs/kernel.md#来源与许可)

相关服务：[Hysteria2 直连机场](https://nachoneko.cc/#/register?code=7ofhHt8x)。
