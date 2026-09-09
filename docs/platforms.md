# 平台与运行细节

## 平台对照

| 项目 | Linux / OpenWrt | Windows |
| --- | --- | --- |
| 分发 | 单个 musl 二进制，amd64 / arm64 | NSIS 安装包，Windows 10/11 x64，需 WebView2 |
| 启动 | 以 root 权限运行；普通用户使用 `sudo` | 每次启动一次 UAC，关窗进入托盘 |
| 面板 | 浏览器打开 `http://localhost:6161`，默认监听 `0.0.0.0` | 自带窗口，仅监听 `127.0.0.1` |
| 开机自启 | `install.sh` 注册 systemd 服务（OpenWrt 不适用） | 托盘勾选登录任务，登录后免 UAC 进入托盘 |
| 面板升级 / VPS 部署 | 支持 | 不提供，升级使用新安装包 |

配置、内核、易变层和偏好的位置见 [Profile 路径表](profiles.md#文件位置)。面板无鉴权，Linux 局域网访问应限制在可信网络。

## Windows 桌面版

从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 安装 `miao-windows-amd64-setup.exe`。安装到当前用户，安装本身无需管理员；缺少 WebView2 时会引导下载，运行内核时仍须 UAC 提权。

- 关窗进入托盘；单击唤出，双击唤出/收回，托盘“退出”才停止内核。
- “开机自启”使用登录任务而非服务；每次启动检查任务中的 exe 路径，升级后失配会重新注册。
- 更新前先从托盘退出，再安装新版；运行中的 exe 会阻止安装/卸载。
- 托盘“打开日志”指向 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`，超过 8 MB 轮转为一份 `.old`。

## OpenWrt

支持 x86_64 / aarch64，启动时自动检测并安装内核依赖；以 root 运行，无需额外 `sudo`。`install.sh` 仅支持 systemd，不用于 OpenWrt。

运行文件、易变配置和选择偏好默认放在 tmpfs，减少高频操作的闪存写入；进程重启时保留，系统重启后回到 YAML/生成配置的默认值。稳定配置和节点绑定仍持久保存。

## 卸载

Linux 卸载会删除服务、程序、`/etc/miao`、运行目录、残留内核进程及 `sing-tun`，**先备份配置**：

```bash
curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/remove.sh | sudo bash
```

源码目录中也可执行 `sudo bash remove.sh`，加 `-y` 跳过确认。Windows 先从托盘退出，再从系统设置卸载。

## PWA

Chrome/Edge 打开 `http://localhost:6161` 后可用浏览器的安装入口获得独立窗口和启动器图标。直接通过局域网 HTTP IP 访问不属于安全上下文，不能使用该安装入口。

部分启动器（如 Hyprland 的 hyprlauncher）不显示浏览器 PWA 图标时，检查 `~/.local/share/icons/hicolor/index.theme` 是否存在并声明各尺寸目录；浏览器可能只写入 PNG。补齐后重启启动器。
