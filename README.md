<div align="center">
  <img src="frontend-rsbuild/public/icon.svg" width="72" alt="Miao logo" />
  <h1>Miao</h1>
  <p><strong>开箱即用的透明代理</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
  </p>
</div>

Miao 将 sing-box 内核、分流规则和 Web 面板打包为单个可执行文件，通过 TUN 接管整机流量。当前发布 Linux / OpenWrt amd64、arm64 版本；Windows 桌面源码保留，已停止发布安装包。

支持[七种节点协议](docs/kernel.md#能力与裁剪)、订阅管理、自动选节点、自定义分流和 MCP 控制。

<img width="1440" height="1400" alt="Miao 控制面板" src="https://github.com/user-attachments/assets/320dd0bb-f1da-4bf9-99ab-6c04c3c2c95b" />

## 安装与启动

从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 下载二进制及 `.sha256`。私有仓库需访问权限；已登录的 GitHub CLI 可用以下命令，arm64 替换文件名中的 `amd64`：

```bash
gh release download --repo YUxiangLuo/miao \
  --pattern miao-rust-linux-amd64 --pattern miao-rust-linux-amd64.sha256 &&
sha256sum -c miao-rust-linux-amd64.sha256 &&
chmod +x miao-rust-linux-amd64 &&
sudo ./miao-rust-linux-amd64
```

校验通过后再运行，面板地址为 `http://localhost:6161`。OpenWrt 以 root 运行，无需 `sudo`。**面板无鉴权，默认监听所有网卡，只应允许可信设备访问。**

也可使用临时订阅，以“日本最快”启动；正常退出后清理，不改写已有配置：

```bash
sudo ./miao-rust-linux-amd64 --sub 'https://your-subscription-url' JP
```

地区、配置文件与临时运行限制见[配置参考](docs/config.md#命令行启动)。TUN 和 Clash API 不支持多开，请先退出已有实例。

systemd Linux 可[安装为服务](docs/platforms.md#升级与卸载)；OpenWrt 与 Windows 自行构建见平台说明。

## 文档

| 任务 | 入口 |
| --- | --- |
| 配置订阅、节点、分流、定时刷新或 MCP | [配置参考](docs/config.md) |
| 平台差异、升级、卸载、PWA | [平台说明](docs/platforms.md) |
| 构建、测试、发布、生产部署 | [开发指南](DEV_NOTES.md) |
| 修改面板或内核 | [前端开发](frontend-rsbuild/README.md)、[内核维护](docs/kernel.md) |
| 修改配置与运行逻辑 | [状态与事务](docs/runtime-state.md)、[Profile 路径](docs/profiles.md) |

[官网与 FAQ](https://miao.vesein.dev) · [Miao 源码许可](LICENSE) · [内核来源与许可](docs/kernel.md#来源与许可)

相关服务：[Hysteria2 直连机场](https://nachoneko.cc/#/register?code=7ofhHt8x)。
