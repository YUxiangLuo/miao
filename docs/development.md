# 开发指南

HarborGate（兼容二进制名 `miao`）日常在 Linux 下开发、检查与发布（Linux / OpenWrt `amd64`、`arm64` 单二进制），同时保留 `desktop/src-tauri` 供 Windows 本地一键打包桌面端与 NSIS 安装程序。Rust `miao-core` 管理配置、REST/MCP 和内核，`miao-cli` 提供命令行入口，`miao-desktop` 仅负责窗口、托盘与提权（非 workspace 默认成员，日常 Linux 开发无需安装 WebKitGTK），SolidJS 2.0 面板通过本地 HTTP API 操作。源码结构与依赖版本以 workspace 清单和 lockfile 为准。

## 开发入口

所有开发命令从仓库根目录的 `./dev.sh` 进入（Windows 可直接运行 `.\desktop\package.cmd` 打包或在 Git Bash 中执行 `./dev.sh`），`./dev.sh help` 查看参数。Linux 安装和卸载分别使用 `install.sh`、`remove.sh`。辅助文件按用途归档：`scripts/lib/` 为内部构建/发布实现，`scripts/tests/` 为脚本测试与隔离验收，`scripts/sing-box/` 为固定内核来源、补丁与 Go 回归。

| 命令 | 用途 |
| --- | --- |
| `./dev.sh test [过滤参数]` | 无资源核心测试，只需 Rust |
| `./dev.sh test --embedded [过滤参数]` | 构建前端，补齐缺失测试资源，运行带资源测试 |
| `./dev.sh check` | ShellCheck、前端检查与构建、脚本测试、两种资源模式的 fmt/Clippy/Rust 测试 |
| `./dev.sh check core` / `frontend` / `scripts` / `windows` / `desktop` | 单独检查核心、前端、脚本、Windows 目标（Linux 下交叉 Clippy）或桌面壳 |
| `./dev.sh audit` | 前端与 Rust 依赖审计，需要联网和 cargo-audit |
| `./dev.sh types` | 从 Rust 生成前端 API 类型，无需生成资源 |
| `./dev.sh build` | 构建前端及本机 release 程序，按需补齐或更新内核/规则 |
| `./dev.sh build amd64` / `arm64` / `windows-amd64` / `all` | 构建对应平台 CLI 程序（`all` 为两架构 Linux musl） |
| `./dev.sh build desktop`（或 `.\desktop\package.cmd`） | 在 Windows 上构建 Tauri 桌面端并打包 `dist/miao-windows-amd64-setup.exe` |
| `./dev.sh kernel [amd64\|arm64\|windows-amd64\|all]` | 重建固定内核及规则；省略目标时使用本机平台架构 |
| `./dev.sh netns [程序路径]` | Linux 命名空间隔离真实内核验收，默认 `target/release/miao-rust` |

`check scripts` 需要 Bun、ShellCheck、jq；只检查辅助脚本，安装/卸载测试使用临时目录与假命令，不操作本机服务。`check frontend` 只检查面板。

核心测试不依赖 `public/`、`embedded/` 或 Bun。不带 `embedded-assets` 时页面返回 503，内核提取返回明确错误；CLI 默认包含真实资源。带资源测试仅为缺失内核/规则创建测试桩，退出后清理，已有真实资源不被覆盖。测试桩不能用于构建或发布。

普通 `check` 与联网 `audit` 分开执行，发布前两者都须通过。修改内核或构建流程后另执行 `kernel all`；修改网络运行逻辑后另做 `netns` 验收。没有远端自动检查或发布流程。

## 构建、发布与部署

本机构建需要 Rust、Bun，资源缺失或过期时还需 Go、Git、curl。`build --refresh-kernel` 强制重建内核和规则；`./dev.sh kernel amd64 --kernel-only` 仅更新指定内核。内核使用固定源码与工具链，两个目标共用一次源码准备、Go 回归和规则编译，见[内核维护](kernel.md)。

`build` 产物为 `target/release/miao-rust`；musl 产物位于 `target/x86_64-unknown-linux-musl/release/miao-rust` 或 `target/aarch64-unknown-linux-musl/release/miao-rust`。构建固定使用仓库的 `target/`，不受 `CARGO_TARGET_DIR` 影响，避免发布时取到旧产物。musl 构建须安装 `zig`、`cargo-zigbuild` 和对应 Rust target，缺失时命令提前报错。

发布流程：

```bash
./dev.sh version vX.Y.Z       # 只修改本地 Cargo.toml/Cargo.lock
./dev.sh check
./dev.sh audit
# 审查并提交版本修改
./dev.sh release              # 干净 HEAD 下重建两架构 musl，准备 dist/vX.Y.Z/
./dev.sh release --publish    # 核验产物后创建/推送 tag，通过 gh 上传 Release
```

`version` 离线更新 workspace 版本并保留依赖锁定；失败时恢复原有 `Cargo.toml` 和 `Cargo.lock`。准备阶段不提交、打 tag 或上传。`dist/vX.Y.Z/release-manifest.json` 是唯一的本地发布清单，绑定 HEAD、版本、两架构二进制的大小与 SHA-256、内核元数据和规则来源；发布拒绝缺失、改动或来自其他提交的产物。

发布目标为 `origin` 唯一的 GitHub 推送地址，tag 检查、推送和附件上传使用同一仓库，需已登录 github.com 的 `gh`。已有同名远端 tag 必须指向同一提交；GitHub 查询失败时在推送前停止。`--draft` 创建或更新草稿，不能用于已公开的 Release；不加该参数时，已有草稿会在上传和附件清理成功后公开。

Release 上传 `miao-rust-linux-amd64`、`miao-rust-linux-arm64` 及各自同名的 `.sha256` 文件，源码 ZIP/TAR.GZ 由 GitHub 根据 tag 自动生成。清单不上传。准备阶段按 `sha256sum` 标准格式生成校验文件，发布前核对其内容与二进制一致；旧暂存目录需重新执行 `release` 补齐。重复发布会在四个附件全部上传成功后删除该 Release 的其他附件。安装与自升级继续使用 [GitHub 生成的 SHA-256 摘要](https://docs.github.com/en/rest/releases/releases)，`.sha256` 附件供手动校验及旧版客户端升级使用。

### Arch 生产实例

**本机 systemd miao 是出网依赖，普通开发不得停服或重启。** 不要裸跑构建出的程序或 `cargo run`，它们会争用 TUN、Clash API 和面板端口。

程序为 `/usr/local/bin/miao`，配置为 `/etc/miao/config.yaml`，运行目录为 `/tmp/miao-sing-box`，面板为 `http://localhost:6161`。部署须明确授权；安装脚本会停服、替换、重启，期间短暂断网：

```bash
./dev.sh build
sudo cp /usr/local/bin/miao "/tmp/miao.$(date +%s).backup"
sudo bash install.sh ./target/release/miao-rust
```

只读排查用 `systemctl status miao`、`sudo pgrep -x miao` 或 `sudo ss -tlnp 'sport = :6161'`。`/api/status` 的 `data.pid` 是内核子进程。卸载会删除 `/etc/miao`，先备份配置。

### Windows 打包与交叉检查

日常在 Linux 修改 Rust 代码后，安装 `mingw-w64-gcc` 并执行 `rustup target add x86_64-pc-windows-gnu`，即可运行 `./dev.sh check windows` 在 Linux 本机完成 Windows 条件编译与 Clippy 检查，无需切换到 Windows 或安装 WebKitGTK。

在 Windows 真机打包需安装 Git for Windows、Rust（MSVC）、VS C++ 生成工具、Bun 与 Go。PowerShell 执行 `.\desktop\package.cmd`（或 Git Bash 执行 `./dev.sh build desktop`，加 `--refresh-kernel` 强制重建内核与规则），产物固定输出到 `dist/miao-windows-amd64-setup.exe` 与同名 `.sha256`。重编或覆盖安装前先从托盘退出运行中的 `miao.exe`。维护桌面端时保留以下约束：

- 参数先校验再提权；`Global\` 互斥量（无权限时回退 `Local\`）防多开，已有实例不再触发 UAC。
- 内核使用独立进程组与 `KILL_ON_JOB_CLOSE` Job Object，停止先发 `CTRL_BREAK_EVENT`，超时强杀并清理 `sing-tun`；配置变更走停止/启动回滚。
- 面板默认仅监听 loopback，端口冲突可回退随机端口；不增加 Tauri 远程 IPC，Windows 不编入 VPS 部署、自替换升级或 OpenWrt 安装器（路由保持 404）。
- 自启通过最高权限 `Miao` 登录计划任务加 `--minimized` 实现，启动时自动修复失配路径；升级 `@tauri-apps/cli` 时须同步核对 [`installer.nsi`](../desktop/src-tauri/nsis/installer.nsi) 中的两处定制钩子。

## 测试隔离与维护约定

修改 core 前读[状态与事务](runtime-state.md)、[Profile 路径](profiles.md)；前端类型、mock 和浏览器规则见[前端开发](../frontend-rsbuild/README.md)。路径只能取自已解析的 `AppState`。

普通测试使用临时目录、假内核与 localhost 订阅。`spawn_server` 设置 `skip_extract: true`，注入隔离配置、运行目录与 `volatile_path`；不得读写生产路径或启动真实 TUN。workspace 默认成员为 `miao-core` 与 `miao-cli`，Linux 下直接运行不带 `--workspace` 的 `cargo test` / `cargo clippy`，避免额外编译桌面壳。

`netns` 需要真实内核、Bun、curl、OpenSSL、iproute2、util-linux、sysctl 和 root/sudo；须先构建待测程序，并通过 `./dev.sh kernel` 准备本机架构的 `embedded/sing-box-host`。它在独立网络/挂载/PID 命名空间验证启动、重载、规则/DNS 配置、策略回退、停止与 TUN 清理。对端命名空间模拟 DNS 和订阅服务器，验证分流/全局模式冷启动时，失效手动节点不妨碍订阅失败后的自动恢复与手动刷新。

工作室验收使用两个客户端命名空间、一个真实 VLESS 服务端和一个带认证的 SOCKS5 服务端，检查来源 IP 绑定、TCP/UDP 转发、国内域名/IP/内网直连、每个节点的海外 DoH、切换默认节点、节点失联/删除/恢复，以及 rule/global 热切换。DoH 证书在隔离环境临时生成，仅由测试 Miao 进程信任。总超时 120 秒，退出销毁环境；不接触公网，不代表真实线路带宽或 OpenWrt 真机验收。

状态查询微基准：`cargo test -p miao-core --locked --no-default-features --release benchmark_status_projection -- --ignored --nocapture`。只测空配置及 100/10000 个节点和规则的内部状态投影，不含 HTTP/内核查询；耗时不作为测试断言。

服务端 HTTP client 使用 `no_proxy()`；VPS 凭据走 stdin，主机密钥用 `accept-new`，Hysteria2 固定版本与官方校验清单一起验证。
