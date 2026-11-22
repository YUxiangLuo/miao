# 平台与运行细节

项目主力面向 Linux / OpenWrt（`x86_64`、`aarch64` 单个 musl 二进制），同时提供 Windows `x86_64` 桌面版与 CLI 的源码一键打包支持。TUN 需要 root / 管理员权限。

| 项目 | Linux / OpenWrt | Windows 桌面版 |
| --- | --- | --- |
| 分发形态 | Release 单文件 `harbor-linux-{amd64,arm64}` | 本地打包 `dist/harbor-windows-amd64-setup.exe` |
| 面板监听 | 默认 `0.0.0.0:6161`（局域网可访问） | 默认 `127.0.0.1:6161`（端口冲突自动回退） |
| 权限与单实例 | root / `sudo`；端口与 TUN 互斥 | 启动经 UAC `runas`；命名互斥量防多开并唤出已有窗口 |
| 开机自启 | systemd 服务（OpenWrt 按系统方式配置） | 托盘勾选 `Harbor` 登录任务，登录后免 UAC 进托盘 |
| 应用内升级 / VPS 部署 | 提供 | 不提供（安装包覆盖升级） |

## 访问与持久化

**面板和 MCP 无鉴权，只应允许可信设备访问。** 文件位置、配置 Profile 和持久化差异见 [Profile 路径表](profiles.md#文件位置)。

## 下载与校验

使用已登录的 GitHub CLI（`gh`）下载并校验 Release 二进制，需要 Bash 和 `sha256sum`。私有仓库要求账号具有访问权限。以下以 amd64 为例，arm64 替换文件名中的 `amd64`：

```bash
repo=YUxiangLuo/miao
asset=harbor-linux-amd64
tag=$(gh release view --repo "$repo" --json tagName -q .tagName) &&
gh release download "$tag" --repo "$repo" --pattern "$asset" --pattern "$asset.sha256" &&
sha256sum -c "$asset.sha256"
```

也可用浏览器下载同一 Release 的程序和同名 `.sha256` 文件，放在同一目录后执行 `sha256sum -c harbor-linux-amd64.sha256`。

校验显示 `OK` 后，按 [README](../README.md#安装与启动) 启动。OpenWrt 缺少 `gh` 时可在另一台电脑完成下载与校验，再传入设备。

安装脚本的自动下载和应用内升级请求未鉴权的 GitHub 地址，不会继承本机 `gh` 登录。无法访问私有 Release 时，使用上述方式取得本地二进制后安装。

## OpenWrt

启动时自动检测并安装内核依赖。以 root 直接运行；`install.sh` 仅支持 systemd，不用于 OpenWrt。高频状态默认放在 tmpfs，进程重启时保留、系统重启后清空；稳定配置和节点绑定持久保存。

## 升级与卸载

systemd Linux 在源码目录用 `sudo bash install.sh /path/to/harbor-linux-amd64` 安装或升级，arm64 替换文件名；会短暂断网。本机生产实例遵循[专用流程](development.md#arch-生产实例)。

不传本地文件时，安装脚本下载最新 Release，并使用 GitHub 元数据中的 SHA-256 摘要校验，需要 curl 或 wget、jq、sha256sum。当前应用内升级也使用该摘要；Release 同时提供 `.sha256` 附件，供手动校验及旧版面板升级使用。

在源码目录执行 `sudo bash remove.sh` 卸载，`-y` 跳过确认。它会删除服务、程序及升级残留、`/etc/harbor`、运行目录及残留内核/TUN，**先备份配置**。Windows 升级或卸载前先从托盘退出，再运行新安装包覆盖安装或从系统设置卸载。

## Windows 桌面版打包与使用

仓库 Release 仅上传 Linux 二进制；Windows 下通过源码直接生成 NSIS 安装包：

```powershell
.\desktop\package.cmd        # PowerShell / CMD
./dev.sh build desktop       # Git Bash
```

构建自动准备前端、定制 `sing-box` 内核与规则集，并将安装包输出到 `dist/harbor-windows-amd64-setup.exe`（附 `.sha256`；仅需 CLI 时运行 `./dev.sh build`，产物为 `target/release/harbor-cli.exe`）。

- **安装与提权**：安装到当前用户目录（`%LOCALAPPDATA%\Harbor`），安装过程无需管理员，缺少 WebView2 时自动引导下载；启动程序时检测管理员权限并通过 UAC 提权，已有实例运行时自动唤出原窗口而不再弹 UAC。
- **托盘与开机自启**：关闭窗口隐藏到系统托盘，左键单击唤出、双击切换显隐，右键「退出」才停止内核。托盘勾选「开机自启」会注册最高权限的 `Harbor` 登录计划任务（带 `--minimized`），启动时若检测到路径失配会自动修复；「打开日志」定位 `%LOCALAPPDATA%\io.github.yuxiangluo.harbor\harbor.log`（超过 2 MB 轮转为 `.old`）。
- **内核生命周期与覆盖升级**：内核子进程置于独立进程组并绑定 `KILL_ON_JOB_CLOSE` Job Object，优雅停止发送 `CTRL_BREAK_EVENT`，异常退出后自动清理残留 `sing-tun` 网卡；配置变更走停止 → 写盘 → 重启并保留失败回滚。安装/卸载钩子检测到 `harbor.exe` 仍在运行时会提示先从托盘退出；卸载勾选删除应用数据时一并清理 `%LOCALAPPDATA%\io.github.yuxiangluo.harbor` 与 `%TEMP%\harbor-sing-box`。

## PWA

Chrome/Edge 打开 `http://localhost:6161` 可安装为独立窗口。局域网 HTTP IP 不属于安全上下文，不能使用该安装入口。

启动器缺少 PWA 图标时，检查 `~/.local/share/icons/hicolor/index.theme` 是否存在且声明各尺寸目录；浏览器可能只写入 PNG，补齐后重启启动器。
