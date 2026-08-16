# 开发调试笔记

本机是 Arch。**`feat/windows-tauri` 是 Windows 线**，不合回 master。在这台 Linux 上按同一套产品哲学做 Tauri 桌面版。命令以仓库根目录为基准。

## 要守住的哲学

Linux 版是：下载一个文件，`sudo` 跑，浏览器打开，TUN 接管流量，面板就是产品。Windows 只换外壳，不换产品。

| 守住 | 不要做成 |
| --- | --- |
| 同一套 `miao-core` + 同一块 React 面板 | 用 Tauri invoke 重写 API，或再做一份前端 |
| 没配置就进引导页 | 安装向导、强制落盘 |
| TUN 透明代理 | 系统代理（WinINET）当主路径 |
| Tauri 只当自带浏览器，打开 `http://127.0.0.1:<port>` | Linux / OpenWrt 也改成 Tauri |
| 一次 UAC 对标一次 `sudo` | 第一版就上服务模式 / 免 UAC |
| 内核钉 1.13.x，Wintun 已在 sing-box 里 | 旁边再塞一份 `wintun.dll`，或跟漂到 1.14 |
| Windows 构建不编 VPS、不编换进程升级 | 把 Linux 的 `exec` 热更原样搬过去覆盖正在跑的 exe |

本机 **正在跑的 systemd miao 不要停、不要重启** 来「试 Windows」。这台机器出网被它的 TUN 管着，一停 GitHub 就没了；也没有 Win10 真机验 TUN。Windows 工作用静态检查和 CI 编译代替真机。

## 仓库怎么切

```
crates/miao-core      库：面板、配置事务、启停内核
crates/miao-cli       Linux/OpenWrt 入口，二进制名仍是 miao-rust
desktop/src-tauri     Tauri 2 壳，二进制名 miao；workspace 成员但不是 default-member
frontend/             唯一一份面板
public/               vite 构建产物，被 include_str! 嵌进 core
embedded/             sing-box + srs，不入库
```

`cargo test` / `cargo clippy`（不加 `--workspace`）只打 core + cli，和 CI 的 Linux quality 一致。不要 `cargo test --workspace`，那会去编桌面壳、拉 webkit。

## 日常命令

```bash
# 前端（改面板时）
bun run --cwd frontend dev
bun run --cwd frontend test
bun run --cwd frontend lint

# 默认成员：Linux 行为必须保持绿
cargo test --locked --all-targets
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings

# Windows 静态门禁（本机可跑，不启动任何代理）
cargo check -p miao-core --target x86_64-pc-windows-gnu
cargo check -p miao-desktop          # 本机有 webkit2gtk 才能过；不要 cargo run

# 内核（只写 embedded/，不要随后 extract 到 /tmp/miao-sing-box）
./scripts/build-embedded.sh                         # Linux 本机架构
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh

# 前端改完必须重新嵌入
./scripts/build-frontend.sh

# 全量 Linux 发布构建
./build.sh
./release.sh vX.Y.Z

gh run list --branch feat/windows-tauri --limit 5
gh run watch <id> --exit-status
```

**禁止**（为了做 Windows 而破坏本机 Linux 实例）：

```bash
sudo systemctl stop miao
sudo ./target/release/miao-rust
cargo run -p miao-desktop
cargo run -p miao-cli
# 以及任何会往 /tmp/miao-sing-box 抽内核的测试（spawn_server 测试必须 skip_extract）
```

**前端改动只跑 `cargo build` 不会生效**：`include_str!` 嵌的是 `public/index.html`。改 JSX/CSS 后先 `./scripts/build-frontend.sh`。

**fresh clone**：`embedded/` 不入库，`include_bytes!` 引用它们。先 `./scripts/build-embedded.sh` 或 `./build.sh`。CI quality 用 stub 文件绕过，本地别学。Windows `include_bytes!` 走 `embedded/sing-box-windows-amd64.exe`；没有这份文件时，交叉 `check` 也会挂。本机若只有 stub，够编译、不够当真内核。

## 在 Arch 上怎么「做」Windows

没有真机，验收用语只能是：测试绿、clippy 过、`windows-gnu` check 过、CI 的 `windows-latest` **编过**桌面壳。不要写「已在 Windows 上验证 TUN」。

交叉 check 需要 MinGW：

```bash
sudo pacman -S --needed mingw-w64-gcc
rustup target add x86_64-pc-windows-gnu
# 若链接器找不到：
# export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
```

`desktop` 在 Linux 上能 `cargo check` 是因为装了 webkit2gtk。CI 的 Ubuntu rust job **不编** `miao-desktop`；Windows 壳由 `windows-latest` job 编，且必须先下载 frontend 产物——`miao-core` 编译期要读 `public/index.html`。

Windows 构建里故意拿掉的东西（管理员进程里不该有）：

- `POST /api/upgrade` 整条换 exe 实现
- `POST /api/vps/deploy` 和 askpass
- OpenWrt apk/opkg 安装器
- Tauri 对 `http://127.0.0.1:*` 的远程 IPC（面板不用 invoke）

Linux CLI 这些能力还在。

## 本机 Linux 实例（别动它）

这台机器日常跑的是 **systemd 服务**：`/usr/local/bin/miao`，`WorkingDirectory=/etc/miao`，配置 `/etc/miao/config.yaml`，运行时 `/tmp/miao-sing-box`，面板 `http://localhost:6161`。做 Windows 改动时把它当「生产依赖」，不是调试对象。

如果以后有人要调 **Linux** 二进制（不是现在这条 Windows 任务）：

```bash
cargo build --locked --release
sudo systemctl stop miao
sudo nohup ./target/release/miao-rust > /tmp/miao-dev.log 2>&1 &
sudo bash install.sh ./target/release/miao-rust
```

注意：

- 别用 `pgrep -f 'target/release/miao-rust'`，会误匹配 `sudo nohup` 包装进程。用 `sudo pgrep -x miao-rust` 或 `sudo ss -tlnp 'sport = :6161'`
- 后端跑在 root 下，普通用户 pgrep 看不到
- `api/status` 的 `data.pid` 是 **sing-box 子进程**，不是后端
- 每次启动都会重释放内嵌文件到运行时目录；`cache.db` / `config.json.cache` 有意保留
- 出网被 TUN 接管：**后端一停，GitHub 就没了**。`install.sh` 在线下载在这台机器上测不了；用离线路径。`remove.sh` 会删 `/etc/miao`，动之前先备份
- git push 用 SSH（`origin` 是 `git@github.com:...`）。HTTPS + gh token 缺 `workflow` scope，推不动 workflow 文件

## Windows 路径与行为（写代码时对照）

| 项 | 实现 |
| --- | --- |
| 配置 | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\config.yaml` |
| 日志 | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`（GUI 没控制台） |
| 上次节点 | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\.last_proxy`（不要再放 Temp） |
| 内核运行时 | `%TEMP%\miao-sing-box` |
| 面板绑定 | `127.0.0.1` |
| 单实例 | 命名 mutex 优先 `Global\io.github.yuxiangluo.miao`（跨会话，防快速用户切换双开抢 TUN），无权时退 `Local\`；未提权先 `OpenMutexW`，已有实例不再 UAC；已有实例的窗口按标题 + 进程镜像（miao.exe）双重确认 |
| 停核 | `CREATE_NEW_PROCESS_GROUP` + `CTRL_BREAK`；超时再杀并只清 `sing-tun`（本机/CI 不要真执行） |
| 提权 | 确认无实例后再 `ShellExecuteW runas`；取消则 MessageBox |
| 端口 | `port_fallback`：6161 被占时桌面壳改绑随机端口，不再直接退出 |
| 内核看门狗 | 崩溃自动拉起（2s 巡检、1–16s 退避、最多 5 次），放弃时写 `config_warning`；有意启动/停核先递增 `sing_generation`，spawn 不覆盖仍活着的子进程 |
| 日志轮转 | `miao.log` 超 8 MB 时启动改名 `.old`（单份滚动） |
| 进程规则文案 | `/api/status` 的 `platform` 决定 placeholder（`qbittorrent.exe`） |

TUN JSON：`auto_route` + `strict_route`，`interface_name` 仍是 `sing-tun`。`cfg(target_os = "linux")` 才写 `auto_redirect`。不要写 1.14 才有的 `dns_mode`。

## 前端调试（agent-browser）

本机面板仍是那份 systemd 上的 Linux 实例，可以用来看 **共享 UI**（规则、节点、布局），不能用来验 UAC / WebView2 / WinTun。

- `agent-browser open <url>` / `eval '<js>'` / `screenshot [--full] <path>`；`set viewport <w> <h>`
- `eval` 跨调用保留 JS 上下文，同名 `const` 会炸，用 IIFE
- React 受控组件要走原生 setter + `input`/`change` 事件
- 首页固定高度，左右列内部滚；活跃链接区恒高 240px 且始终渲染
- 移动端 ≤840px 由 `isDesktop` 门控，不要再加 CSS `display:none`

## 写测试

**前端**：`<details>` 折叠看 `open` 属性；RuleModal 关闭时不在 DOM；密码 ≥8 位；App 集成测试 mock `/api/status|subs|nodes|rules|version` 和 Clash 端点。Windows 相关：`upgrade_supported: false` 时版本条不是按钮；`vpsSupported: false` 时没有 VPS tab；`platform: 'windows'` 时进程规则 placeholder 换成 exe 路径。

**后端**：只测纯函数、校验失败、读路径、不触网的 handler。成功写路径会起真实 sing-box，单测别碰。`spawn_server` 必须 `skip_extract: true`，且不要写 `/tmp/miao-sing-box`。Windows 专属路由测试用 `#[cfg(windows)]` 断言 `/api/upgrade`、`/api/vps/deploy` 是 404。

配置变更链路没变：`config_update` 锁 → 改克隆 → `apply_config_change`（原子写 → 生成 → `sing-box check` → 热重启）。面板不直接碰内核，走 Clash API。`route_mode` 仍是会话级。

## CI

`ci.yml` 在这条分支的 push、以及 PR 上跑：

1. Frontend quality
2. Rust quality（default-members + `cargo check -p miao-core --target x86_64-pc-windows-gnu`）
3. Windows（`windows-latest`：`cargo test -p miao-core`，再 `cargo build -p miao-desktop`；内核用 stub，**不跑 exe、不开 TUN**）

打 `v*` tag 或手动跑 **Build Release**：编真内核，产物只挂在该次 run 的 Artifacts（`miao-windows-amd64`），不上 GitHub Releases 页面。本机不出安装包。

## 脚本

- `install.sh` / `remove.sh` 仍是 Linux systemd 的事，提交前 `shellcheck`
- `build-embedded.sh` 的 `MIAO_TARGET=windows-amd64` 只多编一份 Windows 内核，也会编 host 规则编译器用的 `sing-box-host`，不要拿它去抽本机正在跑的实例
