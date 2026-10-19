<div align="center">
  <img src="frontend-rsbuild/public/icon.svg" width="72" alt="Miao logo" />
  <h1>MiaoBox</h1>
  <p><strong>开箱即用的透明代理</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
  </p>
</div>

Miao 将 sing-box 内核、分流规则和 Web 面板打包为单个可执行文件，通过 TUN 接管整机流量。支持 Linux / OpenWrt，提供 amd64、arm64 两种架构。

支持订阅管理、按地区自动选最快节点、自定义分流、定时刷新和 MCP 控制。节点协议见[内核能力](docs/kernel.md#能力与裁剪)。

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

临时订阅与启动参数见[配置参考](docs/config.md#命令行启动)；systemd Linux 的服务安装、升级和卸载见[平台说明](docs/platforms.md#升级与卸载)。

## 从源码构建

需要 Rust、Bun；首次构建还需 Go、Git、curl 准备内核与规则。在仓库根目录执行：

```bash
./dev.sh build
```

产物为 `target/release/miao-rust`。使用当前源码安装或更新 systemd 服务：

```bash
sudo bash install.sh ./target/release/miao-rust
```

安装保留现有配置并重启服务，期间会短暂断网。交叉编译、测试和发布见[开发指南](docs/development.md)。

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
