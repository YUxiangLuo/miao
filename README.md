<div align="center">
  <img src="frontend-rsbuild/public/icon.svg" width="72" alt="Miao logo" />
  <h1>Miao</h1>
  <p><strong>开箱即用的透明代理</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
  </p>
</div>

Miao 将 sing-box 内核、分流规则和 Web 面板打包在一起，用 TUN 接管整机流量。Linux / OpenWrt 分发单个可执行文件，Windows 提供带托盘的桌面程序；在面板中配置订阅、节点和规则，支持深浅双主题。

客户端内核支持 Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC，裁剪与构建方式见[内核文档](docs/kernel.md)。

<img width="1440" height="1400" alt="Miao 控制面板" src="https://github.com/user-attachments/assets/320dd0bb-f1da-4bf9-99ab-6c04c3c2c95b" />

## 安装

### Linux / OpenWrt

```bash
mkdir -p ~/miao && cd ~/miao
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao
chmod +x miao
sudo ./miao
```

打开 `http://localhost:6161`。arm64 将文件名中的 `amd64` 改为 `arm64`；OpenWrt 已是 root 时直接运行 `./miao`。

也可使用临时订阅，以“日本最快”启动：

```bash
sudo ./miao --sub 'https://your-subscription-url' JP
```

支持 `HK/JP/TW/SG/US`，省略地区时手动选择；临时运行不改写已有配置。不要与已有实例同时启动，详见[命令行用法](docs/config.md#命令行启动)。

systemd Linux 可直接安装为服务：

```bash
curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/install.sh | sudo bash
```

### Windows

从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 下载 `miao-windows-amd64-setup.exe`（Windows 10/11 x64）。启动时确认 UAC，关窗进入托盘；WebView2、自启、升级和卸载见[平台说明](docs/platforms.md)。

## 文档

| 需要做什么 | 文档 |
| --- | --- |
| 配置订阅、节点、规则或 MCP | [配置参考](docs/config.md) |
| 查看平台差异、PWA、升级与卸载 | [平台说明](docs/platforms.md) |
| 从源码构建、测试或部署 | [开发指南](DEV_NOTES.md) |
| 修改面板或定制内核 | [前端开发](frontend-rsbuild/README.md)、[内核维护](docs/kernel.md) |

[官网与 FAQ](https://miao.vesein.dev) · [Miao 源码许可](LICENSE) · [内嵌内核来源与许可](docs/kernel.md#来源与许可)
