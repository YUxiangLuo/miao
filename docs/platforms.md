# 平台与运行细节

当前发布 Linux / OpenWrt amd64、arm64 二进制。Windows 桌面源码保留，安装包需[自行构建](../DEV_NOTES.md#windows-开发)。

## 平台对照

| 项目 | Linux / OpenWrt | Windows 自行构建 |
| --- | --- | --- |
| 分发 | 单个 musl 二进制 | NSIS 安装包，Windows 10/11 x64、WebView2 |
| 启动 | root 权限；普通用户用 `sudo` | UAC 提权，关窗进入托盘 |
| 面板 | `http://localhost:6161`，监听 `0.0.0.0` | 自带窗口，监听 `127.0.0.1` |
| 自启 | `install.sh` 注册 systemd 服务，OpenWrt 不适用 | 托盘勾选登录任务，登录后免 UAC 进入托盘 |
| 应用内升级 / VPS 部署 | 提供 | 不提供 |

**面板和 MCP 无鉴权，只应允许可信设备访问。** 文件位置和持久化差异见 [Profile 路径表](profiles.md#文件位置)。

## OpenWrt

支持 x86_64/aarch64，启动时自动检测并安装内核依赖。以 root 直接运行；`install.sh` 仅支持 systemd，不用于 OpenWrt。高频状态默认放在 tmpfs，进程重启时保留、系统重启后清空；稳定配置和节点绑定持久保存。

## Windows 桌面版

安装到当前用户，安装本身无需管理员；缺少 WebView2 时引导下载，启动内核仍须 UAC。

- 关窗进入托盘，单击唤出、双击切换显隐；托盘“退出”才停止内核。
- 自启使用登录任务，启动时检查并修复升级后失配的 exe 路径。
- 托盘“打开日志”指向 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`，超过 8 MB 轮转为 `.old`。

## 升级与卸载

Linux 在源码目录用 `sudo bash install.sh /path/to/miao-rust-linux-amd64` 安装或升级，arm64 替换文件名；会短暂断网。本机生产实例遵循[专用流程](../DEV_NOTES.md#arch-生产实例)。

私有仓库资源需使用有权限的浏览器或已登录的 `gh` 下载，见 [README](../README.md#安装与启动)。安装脚本的自动下载和应用内升级仍请求未鉴权的 GitHub 地址，不会继承本机 `gh` 登录；无法访问私有 Release 时使用已下载的本地二进制。

Linux 在源码目录执行 `sudo bash remove.sh` 卸载，`-y` 跳过确认。它会删除服务、程序、`/etc/miao`、运行目录及残留内核/TUN，**先备份配置**。Windows 更新或卸载前先从托盘退出，再安装新包或从系统设置卸载；运行中的 exe 会阻止操作。

## PWA

Chrome/Edge 打开 `http://localhost:6161` 可安装为独立窗口。局域网 HTTP IP 不属于安全上下文，不能使用该安装入口。

启动器缺少 PWA 图标时，检查 `~/.local/share/icons/hicolor/index.theme` 是否存在且声明各尺寸目录；浏览器可能只写入 PNG，补齐后重启启动器。
