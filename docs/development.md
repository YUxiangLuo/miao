# 开发指南

Harbor 日常在 Linux 开发、测试与发布，通过 GitHub Actions 统一产出 Linux / OpenWrt（`amd64`、`arm64`）单二进制与 Windows 桌面 NSIS 安装包。Workspace 包含：

- `crates/harbor-core`：配置、REST/MCP、内核编排与运行状态机（默认成员）
- `crates/harbor-cli`：CLI 入口（默认成员）
- `desktop/src-tauri`：Windows 窗口、托盘与 UAC 提权外壳（非默认成员，Linux 开发无需 WebKitGTK）
- `frontend-rsbuild`：SolidJS 2.0 控制台，构建到 `public/` 后由 Rust 嵌入

## 开发入口

全部开发命令经根目录 `./dev.sh` 执行（`./dev.sh help` 查看完整参数）；`scripts/lib/` 为内部构建发布实现，`scripts/tests/` 为脚本单测与 `netns` 验收，`scripts/sing-box/` 为固定内核补丁与 Go 回归。

| 命令 | 用途 |
| --- | --- |
| `./dev.sh test [过滤]` | 无资源纯 Rust 核心测试（无需 Bun / 内核资源） |
| `./dev.sh test --embedded [过滤]` | 构建前端、按需创建临时测试桩并运行带资源测试 |
| `./dev.sh check [all\|core\|rust\|frontend\|scripts\|windows\|desktop]` | 按范围执行检查；默认 `all`（scripts + frontend + 双资源模式 Rust fmt/Clippy/test） |
| `./dev.sh audit` | 前端与 Rust 依赖审计（需联网与 `cargo-audit`） |
| `./dev.sh types` | 从 Rust serde 模型重新生成 `frontend-rsbuild/src/types/api.ts` |
| `./dev.sh build [native\|amd64\|arm64\|windows-amd64\|desktop\|all]` | 构建前端与 release 产物，缺失或过期时自动准备内核/规则（支持 `--refresh-kernel`、`--skip-frontend`） |
| `./dev.sh kernel [amd64\|arm64\|windows-amd64\|all]` | 重建固定内核与规则（`--kernel-only` 跳过规则） |
| `./dev.sh netns [程序路径]` | Linux 命名空间真实内核隔离验收（默认 `target/release/harbor-cli`） |
| `./dev.sh version vX.Y.Z` | 离线更新 `Cargo.toml` 与 `Cargo.lock` workspace 版本 |
| `./dev.sh release [--push]` | 校验发版状态；加 `--push` 后打 tag 并推送触发 GitHub Actions 发布 |

- `check scripts` 需 Bun、ShellCheck、jq（使用临时目录和假命令，不触碰本机服务）。
- 不带 `embedded-assets` 特性时面板返回 503、内核提取返回明确错误；`test --embedded` 仅为缺失内核/规则生成临时桩并自动清理，绝不覆盖已有真实资源。

## GitHub Actions CI 与发布流程

- **快速 CI（[ci.yml](../.github/workflows/ci.yml)）**：推送或 PR 到 `master` 时并行运行 `scripts`、`frontend`、`rust-linux` 与 `rust-windows-cross`（MinGW-w64 交叉 Clippy）。
- **定时审计（[audit.yml](../.github/workflows/audit.yml)）**：每周一 UTC 03:00（及手动触发）运行 `./dev.sh audit`。
- **多平台发布（[release.yml](../.github/workflows/release.yml)）**：推送 `vX.Y.Z` tag 或手动 `workflow_dispatch`（支持 `draft` 与 `expected_version` 校验）触发。

标准发版命令：

```bash
./dev.sh version vX.Y.Z
git commit -am "chore: bump version to vX.Y.Z"
./dev.sh release --push
```

Release 流水线顺序：**① Preflight**（校验 tag/版本/草稿防覆盖，统一锁定 Go 版本与规则集 SHA）→ **② 门禁与统一前端构建**（`verify-scripts`、`build-frontend`、`verify-rust`）→ **③ 跨平台并行构建**（`build-linux` 用 `cargo-zigbuild` 产出双架构 musl 并做 `--version` 冒烟；`build-windows` 重建内核、跑完整 Windows workspace Clippy/test 并打 NSIS 包）→ **④ 校验与原子发布**（检查 ELF/PE 头、内核规则一致性与 `.sha256`，上传 6 个标准产物；`release-manifest.json` 作为 Artifact 保留 90 天）。

## 本地构建与生产部署

本机构建默认产出 `target/release/harbor-cli`；musl 交叉产物在 `target/{x86_64,aarch64}-unknown-linux-musl/release/harbor-cli`（固定使用仓库 `target/`，需安装 `zig`、`cargo-zigbuild` 及对应 Rust target）。内核构建细节见[内核维护](kernel.md)。

### Arch 生产实例

**本机 systemd `harbor` 是系统出网依赖，普通开发不得停服、重启或裸跑 `cargo run` / 编译产物**（会争用 TUN、Clash API 与面板端口）。

- 路径：程序 `/usr/local/bin/harbor`，配置 `/etc/harbor/config.yaml`，运行目录 `/tmp/harbor-sing-box`，面板 `http://localhost:6161`。
- 只读排查：`systemctl status harbor`、`sudo pgrep -x harbor`、`sudo ss -tlnp 'sport = :6161'`（`/api/status` 的 `data.pid` 为内核子进程）。
- 部署须明确授权（会短暂断网）：

```bash
./dev.sh build
sudo cp /usr/local/bin/harbor "/tmp/harbor.$(date +%s).backup"
sudo bash install.sh ./target/release/harbor-cli
```

## Windows 交叉检查与桌面打包

- **Linux 下交叉检查**：安装 `mingw-w64-gcc` 并执行 `rustup target add x86_64-pc-windows-gnu`，即可运行 `./dev.sh check windows` 完成 Windows 条件编译与 Clippy 校验。
- **Windows 本地打包**：安装 Git for Windows、Rust（MSVC）与 VS C++ 生成工具、Bun、Go 后，在仓库根目录执行 `.\desktop\package.cmd`（PowerShell/CMD）或 `./dev.sh build desktop`（Git Bash，可加 `--refresh-kernel`），产物固定输出到 `dist/harbor-windows-amd64-setup.exe` 及 `.sha256`。覆盖安装前先从托盘退出运行中的 `harbor.exe`。
- **桌面端维护约束**：
  - 参数先校验再提权；`Global\` 互斥量（无权限回退 `Local\`）防多开，已有实例不触发 UAC。
  - `--minimized` 隐藏创建窗口时不得同步设置 `WS_MAXIMIZE`，延迟到首次可见唤出时最大化。
  - 内核运行于独立进程组并绑定 `KILL_ON_JOB_CLOSE` Job Object；停止先发 `CTRL_BREAK_EVENT`，超时强杀并清理 `sing-tun`；配置变更走停止/启动回滚。
  - 面板仅监听 loopback，端口冲突可回退随机端口；不增加 Tauri 远程 IPC；不编入 VPS 部署、自替换升级与 OpenWrt 安装逻辑（路由保持 404），仅允许白名单内的本仓库 HTTPS Release 页经 `explorer.exe` 在默认浏览器打开。
  - 升级 `@tauri-apps/cli` 时同步核对 [installer.nsi](../desktop/src-tauri/nsis/installer.nsi) 中的定制钩子。

## 测试隔离与维护约定

修改 `harbor-core` 前先读[状态与事务](runtime-state.md)与[Profile 路径](profiles.md)；前端约定见[前端开发](../frontend-rsbuild/README.md)。业务路径只能取自已解析的 `AppState`。

- **单元测试**：Linux 下直接跑不带 `--workspace` 的 `cargo test` / `cargo clippy`。测试通过 `spawn_server`（`skip_extract: true`）注入临时配置、运行目录与假内核，严禁读写生产路径或创建真实 TUN。
- **服务端 HTTP 客户端**：统一使用 `no_proxy()`；VPS 凭据走 stdin，主机密钥用 `accept-new`，Hysteria2 固定版本按官方清单校验。
- **`netns` 隔离验收**：需先构建待测程序并通过 `./dev.sh kernel` 准备 `embedded/sing-box-host`（依赖 Bun、curl、OpenSSL、iproute2、util-linux、sysctl 与 sudo）。在独立网络/挂载/PID 命名空间内（总超时 120s，退出即毁，不触公网）验证启停、热重载、订阅故障恢复、以及双客户端 + 真实 VLESS/SOCKS5 服务端下的直连优先、来源 IP 绑定、TCP/UDP、每节点 DoH 与 `rule`/`global` 热切换。
- **状态投影微基准**：`cargo test -p harbor-core --locked --no-default-features --release benchmark_status_projection -- --ignored --nocapture`（测 0 / 100 / 10000 节点与规则的内存投影，不含 HTTP/内核调用）。
