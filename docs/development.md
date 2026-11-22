# 开发指南

Harbor 日常在 Linux 下开发、检查与发布（Linux / OpenWrt `amd64`、`arm64` 单二进制），同时保留 `desktop/src-tauri` 供 Windows 本地一键打包桌面端与 NSIS 安装程序。Rust `harbor-core` 管理配置、REST/MCP 和内核，`harbor-cli` 提供命令行入口，`harbor-desktop` 仅负责窗口、托盘与提权（非 workspace 默认成员，日常 Linux 开发无需安装 WebKitGTK），SolidJS 2.0 面板通过本地 HTTP API 操作。源码结构与依赖版本以 workspace 清单和 lockfile 为准。

## 开发入口

所有开发命令从仓库根目录的 `./dev.sh` 进入（Windows 可直接运行 `.\desktop\package.cmd` 打包或在 Git Bash 中执行 `./dev.sh`），`./dev.sh help` 查看参数。Linux 安装和卸载分别使用 `install.sh`、`remove.sh`。辅助文件按用途归档：`scripts/lib/` 为内部构建/发布实现，`scripts/tests/` 为脚本测试与隔离验收，`scripts/sing-box/` 为固定内核来源、补丁与 Go 回归。

| 命令 | 用途 |
| --- | --- |
| `./dev.sh test [过滤参数]` | 无资源核心测试，只需 Rust |
| `./dev.sh test --embedded [过滤参数]` | 构建前端，补齐缺失测试资源，运行带资源测试 |
| `./dev.sh check` | ShellCheck、前端检查与构建、脚本测试、两种资源模式的 fmt/Clippy/Rust 测试 |
| `./dev.sh check core` / `rust` / `frontend` / `scripts` / `windows` / `desktop` | 单独检查核心、Rust 完整检查、前端、脚本、Windows 目标（Linux 下交叉 Clippy）或桌面壳 |
| `./dev.sh audit` | 前端与 Rust 依赖审计，需要联网和 cargo-audit |
| `./dev.sh types` | 从 Rust 生成前端 API 类型，无需生成资源 |
| `./dev.sh build` | 构建前端及本机 release 程序，按需补齐或更新内核/规则（支持 `--refresh-kernel` / `--skip-frontend`） |
| `./dev.sh build amd64` / `arm64` / `windows-amd64` / `all` | 构建对应平台 CLI 程序（`all` 为两架构 Linux musl） |
| `./dev.sh build desktop`（或 `.\desktop\package.cmd`） | 在 Windows 上构建 Tauri 桌面端并打包 `dist/harbor-windows-amd64-setup.exe` |
| `./dev.sh kernel [amd64\|arm64\|windows-amd64\|all]` | 重建固定内核及规则；省略目标时使用本机平台架构 |
| `./dev.sh netns [程序路径]` | Linux 命名空间隔离真实内核验收，默认 `target/release/harbor-cli` |
| `./dev.sh version vX.Y.Z` | 离线更新 `Cargo.toml` / `Cargo.lock` workspace 版本 |
| `./dev.sh release [--push]` | 校验发版就绪状态；加 `--push` 后打 tag 并推送以触发 GitHub Actions 发布 |

`check scripts` 需要 Bun、ShellCheck、jq；只检查辅助脚本，安装/卸载测试使用临时目录与假命令，不操作本机服务。`check frontend` 只检查面板。

核心测试不依赖 `public/`、`embedded/` 或 Bun。不带 `embedded-assets` 时页面返回 503，内核提取返回明确错误；CLI 默认包含真实资源。带资源测试仅为缺失内核/规则创建测试桩，退出后清理，已有真实资源不被覆盖。测试桩不能用于构建或发布。

## GitHub Actions CI 与发布流程

CI、安全审计与多平台版本发布已全面由 GitHub Actions 驱动：

- **轻量快速 CI（[`.github/workflows/ci.yml`](../.github/workflows/ci.yml)）**：每次推送或 PR 到 `master` 时在 `ubuntu-latest` 上并行执行 `scripts`（ShellCheck + 脚本单测）、`frontend`（lint / typecheck / rstest / build）、`rust-linux`（fmt + 无资源/嵌入式资源 Clippy 与单测）和 `rust-windows-cross`（MinGW-w64 交叉 Clippy 检查 Windows 条件编译）。
- **定时依赖审计（[`.github/workflows/audit.yml`](../.github/workflows/audit.yml)）**：每周一 UTC 03:00 自动（及支持手动触发）执行 `./dev.sh audit`（`bun audit` + `cargo audit`），不阻塞日常 PR。
- **多平台自动发布（[`.github/workflows/release.yml`](../.github/workflows/release.yml)）**：由语义化 Tag 推送（`vX.Y.Z`）或在 Actions 页面手动 `workflow_dispatch`（可选勾选 `draft` 发布为草稿或填写 `expected_version` 防呆校验）触发。

日常发布标准流程：

```bash
./dev.sh version vX.Y.Z       # 离线更新本地 Cargo.toml / Cargo.lock
git commit -am "chore: bump version to vX.Y.Z"
./dev.sh release --push       # 安全核验后打 tag 并推送，触发 GitHub Actions Release
```

（也可以仅推送 `master` 后，在 GitHub 仓库的 **Actions → Release → Run workflow** 中手动触发；选择 `draft = true` 可先生成草稿 Release，审核后再取消草稿重跑即可公开。）

Release 流水线执行顺序：

1. **Preflight（`scripts/lib/ci-release.sh preflight`）**：校验 Tag 与 `Cargo.toml` 版本一致、校验远端 Tag 未指向其他提交、拒绝用草稿覆盖已公开 Release，并统一解析固定的 Go 版本与上游规则集提交（`sing-geoip` / `v2ray-rules-dat`），保证 Linux 与 Windows 嵌入完全一致的规则数据。
2. **前置门禁与统一前端构建**：并行运行脚本检查、前端全检并构建 `public/` 产物，复用给后续所有平台任务。
3. **跨平台并行构建**：
   - `build-linux`（`ubuntu-latest`）：重建 `amd64` + `arm64` 定制 `sing-box` 内核与规则，用 `cargo-zigbuild` 编译双架构 musl 静态二进制，并对 `harbor-linux-amd64 --version` 做原生冒烟校验。
   - `build-windows`（`windows-latest`）：重建 `windows-amd64` 定制内核与规则，执行完整 Windows workspace Clippy 与单测，随后打包 Tauri NSIS 安装包 `harbor-windows-amd64-setup.exe`。
4. **统一装配、完整性核验与发布（`scripts/lib/ci-release.sh publish`）**：调用 `scripts/lib/release-artifacts.mjs assemble & verify` 校验全部 3 份二进制/安装包的 ELF/PE 头、内核与规则来源一致性及 `.sha256` 校验文件，然后原子上传 6 个标准附件（`harbor-linux-amd64`、`harbor-linux-arm64`、`harbor-windows-amd64-setup.exe` 及各自同名 `.sha256`），最后才清理历史废弃附件并将草稿转为正式版；完整构建溯源清单 `release-manifest.json` 保留为 Workflow Artifact（90 天），不污染公开 Release 附件列表。

> 本机构建仍可通过 `./dev.sh build [native|amd64|arm64|windows-amd64|desktop|all]` 完成；修改内核或网络运行逻辑后可按需在 Linux 本机额外执行 `kernel all` 与 `netns` 验收。

## 本地构建与生产部署

本机构建需要 Rust、Bun，资源缺失或过期时还需 Go、Git、curl。`build --refresh-kernel` 强制重建内核和规则；`./dev.sh kernel amd64 --kernel-only` 仅更新指定内核。内核使用固定源码与工具链，两个目标共用一次源码准备、Go 回归和规则编译，见[内核维护](kernel.md)。

`build` 产物为 `target/release/harbor-cli`；musl 产物位于 `target/x86_64-unknown-linux-musl/release/harbor-cli` 或 `target/aarch64-unknown-linux-musl/release/harbor-cli`。构建固定使用仓库的 `target/`，不受 `CARGO_TARGET_DIR` 影响。musl 交叉构建须安装 `zig`、`cargo-zigbuild` 和对应 Rust target，缺失时命令提前报错。

### Arch 生产实例

**本机 systemd harbor 是出网依赖，普通开发不得停服或重启。** 不要裸跑构建出的程序或 `cargo run`，它们会争用 TUN、Clash API 和面板端口。

程序为 `/usr/local/bin/harbor`，配置为 `/etc/harbor/config.yaml`，运行目录为 `/tmp/harbor-sing-box`，面板为 `http://localhost:6161`。部署须明确授权；安装脚本会停服、替换、重启，期间短暂断网：

```bash
./dev.sh build
sudo cp /usr/local/bin/harbor "/tmp/harbor.$(date +%s).backup"
sudo bash install.sh ./target/release/harbor-cli
```

只读排查用 `systemctl status harbor`、`sudo pgrep -x harbor` 或 `sudo ss -tlnp 'sport = :6161'`。`/api/status` 的 `data.pid` 是内核子进程。卸载会删除 `/etc/harbor`，先备份配置。

### Windows 打包与交叉检查

日常在 Linux 修改 Rust 代码后，安装 `mingw-w64-gcc` 并执行 `rustup target add x86_64-pc-windows-gnu`，即可运行 `./dev.sh check windows` 在 Linux 本机完成 Windows 条件编译与 Clippy 检查，无需切换到 Windows 或安装 WebKitGTK。

在 Windows 真机打包需安装 Git for Windows、Rust（MSVC）、VS C++ 生成工具、Bun 与 Go。PowerShell 执行 `.\desktop\package.cmd`（或 Git Bash 执行 `./dev.sh build desktop`，加 `--refresh-kernel` 强制重建内核与规则），产物固定输出到 `dist/harbor-windows-amd64-setup.exe` 与同名 `.sha256`。重编或覆盖安装前先从托盘退出运行中的 `harbor.exe`。维护桌面端时保留以下约束：

- 参数先校验再提权；`Global\` 互斥量（无权限时回退 `Local\`）防多开，已有实例不再触发 UAC。
- 内核使用独立进程组与 `KILL_ON_JOB_CLOSE` Job Object，停止先发 `CTRL_BREAK_EVENT`，超时强杀并清理 `sing-tun`；配置变更走停止/启动回滚。
- 面板默认仅监听 loopback，端口冲突可回退随机端口；不增加 Tauri 远程 IPC，Windows 不编入 VPS 部署、自替换升级或 OpenWrt 安装器（路由保持 404）。
- 自启通过最高权限 `Harbor` 登录计划任务加 `--minimized` 实现，启动时自动修复失配路径；升级 `@tauri-apps/cli` 时须同步核对 [`installer.nsi`](../desktop/src-tauri/nsis/installer.nsi) 中的两处定制钩子。

## 测试隔离与维护约定

修改 core 前读[状态与事务](runtime-state.md)、[Profile 路径](profiles.md)；前端类型、mock 和浏览器规则见[前端开发](../frontend-rsbuild/README.md)。路径只能取自已解析的 `AppState`。

普通测试使用临时目录、假内核与 localhost 订阅。`spawn_server` 设置 `skip_extract: true`，注入隔离配置、运行目录与 `volatile_path`；不得读写生产路径或启动真实 TUN。workspace 默认成员为 `harbor-core` 与 `harbor-cli`，Linux 下直接运行不带 `--workspace` 的 `cargo test` / `cargo clippy`，避免额外编译桌面壳。

`netns` 需要真实内核、Bun、curl、OpenSSL、iproute2、util-linux、sysctl 和 root/sudo；须先构建待测程序，并通过 `./dev.sh kernel` 准备本机架构的 `embedded/sing-box-host`。它在独立网络/挂载/PID 命名空间验证启动、重载、规则/DNS 配置、策略回退、停止与 TUN 清理。对端命名空间模拟 DNS 和订阅服务器，验证分流/全局模式冷启动时，失效手动节点不妨碍订阅失败后的自动恢复与手动刷新。

工作室验收使用两个客户端命名空间、一个真实 VLESS 服务端和一个带认证的 SOCKS5 服务端，检查来源 IP 绑定、TCP/UDP 转发、国内域名/IP/内网直连、每个节点的海外 DoH、切换默认节点、节点失联/删除/恢复，以及 rule/global 热切换。DoH 证书在隔离环境临时生成，仅由测试 Harbor 进程信任。总超时 120 秒，退出销毁环境；不接触公网，不代表真实线路带宽或 OpenWrt 真机验收。

状态查询微基准：`cargo test -p harbor-core --locked --no-default-features --release benchmark_status_projection -- --ignored --nocapture`。只测空配置及 100/10000 个节点和规则的内部状态投影，不含 HTTP/内核查询；耗时不作为测试断言。

服务端 HTTP client 使用 `no_proxy()`；VPS 凭据走 stdin，主机密钥用 `accept-new`，Hysteria2 固定版本与官方校验清单一起验证。
