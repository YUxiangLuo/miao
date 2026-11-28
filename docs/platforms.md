# 平台与运行说明

Harbor 随 GitHub Release 统一发布 Linux / OpenWrt 单文件二进制与 Windows 桌面安装包。由于需创建 TUN 网卡，运行时均需 root 或管理员权限。

| 项目 | Linux / OpenWrt | Windows 桌面版 |
| --- | --- | --- |
| 分发产物 | `harbor-linux-{amd64,arm64}`（静态 musl） | `harbor-windows-amd64-setup.exe`（NSIS 安装包） |
| 默认监听 | `0.0.0.0:6161`（局域网可达） | `127.0.0.1:6161`（端口冲突自动回退） |
| 权限与单实例 | root / `sudo`；TUN 与端口互斥 | 启动经 UAC 提权；命名互斥量防多开并唤出已有窗口 |
| 开机自启 | systemd 服务（OpenWrt 按系统方式配置） | 托盘勾选「开机自启」（最高权限登录计划任务，免 UAC） |
| 更新与 VPS 部署 | 支持应用内自升级与 SSH 部署 VPS | 支持检查更新并在浏览器打开 Release 下载页；不含 VPS 部署 |

## 访问与持久化

**面板和 MCP 无鉴权，只应允许可信网络访问。** 各平台配置文件、易变状态、偏好与日志位置见 [Profile 文件位置](profiles.md#文件位置)。

## 下载与校验

使用 `gh` 下载并校验（`arm64` 替换文件名即可；也可浏览器下载二进制与同名 `.sha256` 后执行 `sha256sum -c`）：

```bash
repo=YUxiangLuo/miao
asset=harbor-linux-amd64
tag=$(gh release view --repo "$repo" --json tagName -q .tagName) &&
gh release download "$tag" --repo "$repo" --pattern "$asset" --pattern "$asset.sha256" &&
sha256sum -c "$asset.sha256"
```

校验通过后按 [README](../README.md#安装与启动) 启动。注意：`install.sh` 自动下载与应用内升级使用匿名 GitHub 请求，不继承本机 `gh` 登录态。

## Linux（systemd）与 OpenWrt

- **systemd 安装 / 升级**：在仓库目录执行 `sudo bash install.sh [本地二进制]`（不传路径则自动下载最新 Release 并按 GitHub 摘要校验 SHA-256，需 `curl`/`wget`、`jq`、`sha256sum`）。脚本先在旧服务运行时完成校验，再短暂停止服务原子替换并重启，启动失败自动回滚。若操作本机生产实例，请遵循[生产部署流程](development.md#arch-生产实例)。
- **systemd 卸载**：`sudo bash remove.sh`（非交互加 `-y`）。会删除服务单元、`/usr/local/bin/harbor` 及升级残留、`/etc/harbor`、`/tmp/harbor-sing-box` 以及残留内核与 `sing-tun`，**执行前先备份配置**。
- **OpenWrt**：以 root 直接运行二进制（不使用 `install.sh`）；启动时自动检测并安装内核依赖。易变状态与日志默认位于 tmpfs（进程重启保留、系统重启清空，避免写穿闪存），稳定配置与节点绑定持久保存。

## Windows 桌面版

直接从 Release 下载 `harbor-windows-amd64-setup.exe` 安装（本地从源码打包见[开发指南](development.md#windows-交叉检查与桌面打包)）：

- **安装与提权**：安装到 `%LOCALAPPDATA%\Harbor`（无需管理员，缺 WebView2 时自动引导下载）；启动时校验参数后通过 UAC 提权，已有实例运行时直接唤出原窗口而不重复弹窗。
- **托盘与自启**：关闭窗口自动收起驻留托盘，左键点击立即唤出并聚焦窗口，右键「退出」才停止内核。勾选「开机自启」注册 `Harbor` 登录计划任务（带 `--minimized`），启动时自动修正迁移后的旧路径；「打开日志」定位 `%LOCALAPPDATA%\io.github.yuxiangluo.harbor\harbor.log`（2 MiB 封顶，单份 `.old` 轮转）。
- **更新与卸载**：在「系统设置」检查新版本后点击「前往下载」会通过默认浏览器打开 GitHub Release 页面。覆盖升级或卸载前请先从托盘右键「退出」；卸载勾选删除数据时一并清理 `%LOCALAPPDATA%\io.github.yuxiangluo.harbor` 与 `%TEMP%\harbor-sing-box`。

## PWA

Chrome / Edge 访问 `http://localhost:6161` 可安装为独立应用窗口（局域网 HTTP IP 非安全上下文，不支持 PWA 安装）。若 Linux 桌面启动器缺图标，确认 `~/.local/share/icons/hicolor/index.theme` 声明对应尺寸目录后刷新启动器即可。
