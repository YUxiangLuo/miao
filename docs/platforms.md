# 平台与运行细节

支持 Linux/OpenWrt x86_64、aarch64，分发单个 musl 二进制。TUN 需要 root 权限，面板默认监听 `0.0.0.0:6161`，浏览器访问 `http://localhost:6161`。

## 访问与持久化

**面板和 MCP 无鉴权，只应允许可信设备访问。** 文件位置、配置 Profile 和持久化差异见 [Profile 路径表](profiles.md#文件位置)。

## 下载与校验

使用已登录的 GitHub CLI（`gh`）下载并校验 Release 二进制，需要 Bash 和 `sha256sum`。私有仓库要求账号具有访问权限。以下以 amd64 为例，arm64 替换文件名中的 `amd64`：

```bash
repo=YUxiangLuo/miao
asset=miao-rust-linux-amd64
tag=$(gh release view --repo "$repo" --json tagName -q .tagName) &&
gh release download "$tag" --repo "$repo" --pattern "$asset" --pattern "$asset.sha256" &&
sha256sum -c "$asset.sha256"
```

也可用浏览器下载同一 Release 的程序和同名 `.sha256` 文件，放在同一目录后执行 `sha256sum -c miao-rust-linux-amd64.sha256`。

校验显示 `OK` 后，按 [README](../README.md#安装与启动) 启动。OpenWrt 缺少 `gh` 时可在另一台电脑完成下载与校验，再传入设备。

安装脚本的自动下载和应用内升级请求未鉴权的 GitHub 地址，不会继承本机 `gh` 登录。无法访问私有 Release 时，使用上述方式取得本地二进制后安装。

## OpenWrt

启动时自动检测并安装内核依赖。以 root 直接运行；`install.sh` 仅支持 systemd，不用于 OpenWrt。高频状态默认放在 tmpfs，进程重启时保留、系统重启后清空；稳定配置和节点绑定持久保存。

## 升级与卸载

systemd Linux 在源码目录用 `sudo bash install.sh /path/to/miao-rust-linux-amd64` 安装或升级，arm64 替换文件名；会短暂断网。本机生产实例遵循[专用流程](development.md#arch-生产实例)。

不传本地文件时，安装脚本下载最新 Release，并使用 GitHub 元数据中的 SHA-256 摘要校验，需要 curl 或 wget、jq、sha256sum。当前应用内升级也使用该摘要；Release 同时提供 `.sha256` 附件，供手动校验及旧版面板升级使用。

在源码目录执行 `sudo bash remove.sh` 卸载，`-y` 跳过确认。它会删除服务、程序及升级残留、`/etc/miao`、运行目录及残留内核/TUN，**先备份配置**。

## PWA

Chrome/Edge 打开 `http://localhost:6161` 可安装为独立窗口。局域网 HTTP IP 不属于安全上下文，不能使用该安装入口。

启动器缺少 PWA 图标时，检查 `~/.local/share/icons/hicolor/index.theme` 是否存在且声明各尺寸目录；浏览器可能只写入 PNG，补齐后重启启动器。
