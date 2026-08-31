# 平台与运行细节

## 平台对照

| | Linux / OpenWrt | Windows |
| --- | --- | --- |
| 分发 | 单个 musl 二进制 | NSIS 安装包（需 WebView2） |
| 提权 | `sudo` | 每次启动一次 UAC |
| 面板 | 浏览器打开 `localhost:6161`，默认听 `0.0.0.0` | 自带窗口，听 `127.0.0.1` |
| 配置 | `/etc/miao/config.yaml` | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\config.yaml` |
| 内核运行时 | `/tmp/miao-sing-box` | `%TEMP%\miao-sing-box` |
| 易变配置 | `/tmp/miao-sing-box/volatile.yaml`（tmpfs） | 应用数据目录（持久） |
| 节点偏好 | Linux：`/etc/miao/.node_select` + `.max_multiplier` + `.last_proxy`（持久）；OpenWrt：运行时 tmpfs | 应用数据目录（持久） |
| 一键升级 / VPS 部署 | 有 | 不编进桌面进程 |
| 开机自启 | `install.sh` → systemd | 托盘勾选（任务计划，登录免 UAC 直进托盘） |

## Windows 桌面版

Win10/11 x64，从 [Releases](https://github.com/YUxiangLuo/miao/releases/latest) 下载 `miao-windows-amd64-setup.exe`：

1. 安装到当前用户，安装本身不需要管理员（缺 WebView2 时安装包会引导下载）
2. 每次启动点一次 UAC（TUN/Wintun 需要管理员）；托盘菜单勾选「开机自启」可免 UAC 自启（任务计划实现，登录后直进托盘）
3. 关窗口进托盘，单击唤出、双击唤出/收回，托盘「退出」才停内核
4. 更新 = 先退出，再装新安装包（面板内无 Windows 一键升级）
5. 日志在 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`（超 8 MB 自动轮转），托盘「打开日志」直达

## OpenWrt

启动时自动检测并安装内核依赖（支持 x86_64 与 aarch64）。运行时文件和选择偏好全部在 `/tmp/miao-sing-box`（tmpfs），不写路由器 flash；面板/进程重启会保留，系统重启后路由模式、节点策略与具体手动节点回到 `config.yaml` / 生成配置的默认值（fail-safe）。

## 干净卸载

`sudo bash remove.sh`：服务、二进制、`/etc/miao`、`/tmp/miao-sing-box`、残留 sing-box 进程与 `sing-tun` 网卡全部清理（`-y` 跳过确认）。Windows 从系统设置卸载，卸载前先从托盘退出。

## 把面板安装成桌面应用（PWA）

面板是 PWA。Chrome/Edge 打开 `localhost:6161` 后地址栏右侧会出现「安装」图标（或菜单 → 安装 Miao），装完有独立窗口和启动器图标，没有浏览器边框。安装入口只在本机 `localhost` 下可用——局域网 IP 访问不是安全上下文，浏览器不允许安装。不想安装的话浏览器书签照旧用。

如果启动器里 PWA 图标不显示（Hyprland 的 hyprlauncher 等）：浏览器安装 PWA 时只往 `~/.local/share/icons/hicolor/<size>/apps/` 丢 PNG，不创建 `index.theme`，而部分启动器会跳过没有 `index.theme` 的图标主题目录。给 `~/.local/share/icons/hicolor/` 补一个声明了各 size 目录的 `index.theme` 即可（修复后所有浏览器 PWA 图标都会出现），重启启动器生效。
