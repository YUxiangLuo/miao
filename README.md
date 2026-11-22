<div align="center">
  <img src="frontend-rsbuild/public/icon.svg" width="72" alt="HarborGate logo" />
  <h1>HarborGate · 海港网关</h1>
  <p><strong>外贸企业跨境网络与多店铺静态 IP 防关联网关</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
  </p>
</div>

HarborGate（底层兼容二进制与路径保持 `miao`）将定制 `sing-box` 内核、大陆/内网直连规则集与 SolidJS 2.0 企业云网关控制台打包为单个可执行文件，专为外贸公司办公网络设计，主力支持 Linux / OpenWrt（`amd64`、`arm64`），同时提供源码一键打包的 Windows 桌面端支持：

- **双粒度防关联隔离**：既支持按员工电脑/手机局域网 IP 分配整机独享专线（读取 `/tmp/dhcp.leases` 自动发现办公终端，单事务无漏流切换），也支持按需开放局域网多店铺专属混合代理端口（如 `:20101` HTTP/SOCKS5 → 美区静态住宅 IP），供指纹浏览器、Chrome 多分身及独立社媒客户端同机多店隔离使用，上游静态住宅 IP 凭据 100% 封存在网关内。
- **离线自动熔断保护**：任何锁定独享专线的工位或专属端口，一旦对应专线或其前置中转跳板不可用，网关自动在路由与 DNS 双层原位改写为 `reject` 阻断，严防回退公共池引发账号关联；国内企业微信、飞书、ERP 与对公网银直连不受影响。
- **静态住宅链式中转（`detour`）与运维可观测性**：面板原生支持录入海外静态住宅 SOCKS5 并绑定前置跳板节点、外贸业务平台一键分流、三维实时流量审计与 MCP 自动化运维接口。

<details>
<summary>查看控制面板</summary>

<img width="1440" height="1400" alt="Miao 控制面板" src="https://github.com/user-attachments/assets/320dd0bb-f1da-4bf9-99ab-6c04c3c2c95b" />

</details>

## 安装与启动

从 [Release](https://github.com/YUxiangLuo/miao/releases/latest) 下载对应架构的程序和同名 `.sha256` 校验文件；源码 ZIP/TAR.GZ 由 GitHub 自动生成。

| 主机架构（`uname -m`） | 下载文件 |
| --- | --- |
| `x86_64` | `miao-rust-linux-amd64` |
| `aarch64` | `miao-rust-linux-arm64` |

先按[下载与校验](docs/platforms.md#下载与校验)核对 SHA-256，私有仓库需使用有访问权限的账号。启动需要 root 权限，TUN 和 Clash API 不支持多开，请先退出已有实例。

以下以 amd64 为例，arm64 替换文件名；OpenWrt 以 root 直接运行，省略 `sudo`：

```bash
chmod +x ./miao-rust-linux-amd64 &&
sudo ./miao-rust-linux-amd64
```

浏览器访问 `http://<设备IP>:6161`（本机为 `http://localhost:6161`），添加订阅或节点。**面板和 MCP 无鉴权，默认监听所有网卡，只应允许可信设备访问。**

临时订阅与启动参数见[配置参考](docs/config.md#命令行启动)；systemd 服务安装、升级、卸载及 Windows 桌面版说明见[平台说明](docs/platforms.md)。

## 从源码构建

需要 Rust、Bun；首次构建还需 Go、Git、curl 准备内核与规则。在仓库根目录执行：

```bash
./dev.sh build
```

产物为 `target/release/miao-rust`。使用当前源码安装或更新 Linux systemd 服务：

```bash
sudo bash install.sh ./target/release/miao-rust
```

安装保留现有配置并重启服务，期间会短暂断网。交叉编译、测试和发布见[开发指南](docs/development.md)。

### Windows 打包与使用

Windows 端不随 GitHub Release 分发，提供本地一键打包。安装 Git for Windows、Rust（MSVC）与 VS C++ 生成工具、Bun、Go 后，在仓库根目录任选一种方式执行：

```powershell
.\desktop\package.cmd        # PowerShell / CMD（自动调用 Git Bash）
./dev.sh build desktop       # Git Bash
```

打包完成后生成 `dist/miao-windows-amd64-setup.exe`（附同名 `.sha256`）。直接运行安装包即可安装到当前用户目录（无需管理员，缺 WebView2 自动引导下载）；启动时经 UAC 提权创建 TUN 并驻留系统托盘（仅监听本机 `127.0.0.1`，支持勾选开机免 UAC 自启与打开日志）。覆盖升级或卸载前，请先在托盘图标右键选择「退出」。

## 文档

| 任务 | 入口 |
| --- | --- |
| 配置订阅、节点、分流、定时刷新或 MCP | [配置参考](docs/config.md) |
| 平台差异、升级、卸载、PWA | [平台说明](docs/platforms.md) |
| 构建、测试、发布、生产部署 | [开发指南](docs/development.md) |
| 修改面板或内核 | [前端开发](frontend-rsbuild/README.md)、[内核维护](docs/kernel.md) |
| 修改配置与运行逻辑 | [状态与事务](docs/runtime-state.md)、[Profile 路径](docs/profiles.md) |

[Miao 源码许可](LICENSE) · [内核来源与许可](docs/kernel.md#来源与许可)

相关服务：[Hysteria2 直连机场](https://nachoneko.cc/#/register?code=7ofhHt8x)。
