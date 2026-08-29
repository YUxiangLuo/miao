<div align="center">
  <img src="frontend-rsbuild/public/icon.svg" width="72" alt="Miao logo" />
  <h1>Miao</h1>
  <p><strong>开箱即用的透明代理</strong></p>
  <p>
    <a href="https://github.com/YUxiangLuo/miao/releases/latest"><img src="https://img.shields.io/github/v/release/YUxiangLuo/miao?style=flat-square" alt="Release" /></a>
  </p>
</div>

Miao 把 sing-box 内核、geo 分流规则和 Web 控制面板编进同一个可执行文件。TUN 接管整机流量，浏览器打开面板即完成配置（深色 / 浅色双主题）。Linux / OpenWrt 上是 `sudo` 即跑的单二进制，Windows 上是带系统托盘的桌面程序。

<img width="1440" height="1400" alt="image" src="https://github.com/user-attachments/assets/320dd0bb-f1da-4bf9-99ab-6c04c3c2c95b" />


## 安装

### Linux / OpenWrt

```bash
mkdir -p ~/miao && cd ~/miao
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao  # arm64 换文件名
chmod +x miao && sudo ./miao
```

```bash
# 或者一键安装为systemd服务
curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/install.sh | sudo bash

# 完全卸载服务和任何痕迹
curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/remove.sh | sudo bash
```

### Windows

从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 下载 `miao-windows-amd64-setup.exe`（Win10/11 x64，需 WebView2）。每次启动点一次 UAC；关窗进托盘；托盘勾选「开机自启」免 UAC 自启（每次启动自动校验自启任务指向的 exe 路径，升级后指向旧版会自修复）。

## 文档

- 官网：<https://miao.vesein.dev>（特性、设计哲学、架构、FAQ）
- 配置参考与 MCP：[docs/config.md](docs/config.md)
- 平台对照与运行细节（Windows / OpenWrt / PWA / 卸载）：[docs/platforms.md](docs/platforms.md)
- 从源码构建：依赖 Bun、Go、Rust、curl，`./build.sh`；开发约定见 [DEV_NOTES.md](DEV_NOTES.md)

[MIT License](LICENSE)
