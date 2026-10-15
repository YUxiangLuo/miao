# 开发指南

Linux/OpenWrt CLI 与保留的 Windows 桌面源码共用 `miao-core` 和 React 面板。Tauri 只负责窗口、托盘和提权，业务统一走本地 HTTP API。当前发布流程只构建 Linux amd64/arm64。

## 代码与文档入口

| 目录 | 职责 |
| --- | --- |
| `crates/miao-core` | 配置、REST/MCP、内核管理；修改前读[状态与事务](docs/runtime-state.md)、[Profile 路径](docs/profiles.md) |
| `crates/miao-cli` | CLI 入口，产物 `miao-rust` |
| `desktop/src-tauri` | Tauri 壳，非 workspace 默认成员 |
| `frontend-rsbuild` | React 面板；见[前端开发](frontend-rsbuild/README.md) |
| `scripts/sing-box` | 固定源码、工具链、补丁和 Go 回归；见[内核维护](docs/kernel.md) |
| `public` / `embedded` | 生成的页面、内核和规则，供 Rust 嵌入，不入库 |

版本与依赖以清单和 lockfile 为准。用户操作见 [README](README.md)。

## 开发检查

在仓库根目录执行。核心检查只需 Rust，前端需 Bun；完整构建另需 Go、Git、curl，依赖审计需 `cargo-audit`。

```bash
./scripts/test-rust.sh  # 核心测试，无需 public/、embedded/ 或 Bun
./scripts/ci.sh quick   # 前端、脚本、两种 Rust 构建检查，复用已有内核
./scripts/ci.sh all     # 加上 amd64/arm64 内核重建与 Go 回归
```

可单独运行 `frontend`、`kernel [target]`、`rust`、`rust-embedded`、`desktop`；`CI_SKIP_AUDIT=1` 跳过联网审计。`rust` 检查不带资源的核心；`rust-embedded` 在前端已构建后检查完整 core + cli。`quick`、`all` 均覆盖两种构建。

`./scripts/test-rust.sh` 等价于 `cargo test -p miao-core --locked --no-default-features --all-targets`，可追加测试过滤参数；API 类型生成也走此模式。不带资源时面板返回 503，内核提取返回明确错误，不写入假资源。`embedded-assets` 默认开启，CLI、桌面和发布构建仍嵌入完整资源。

需要资源集成测试时用 `./scripts/test-rust.sh --embedded`：构建前端，仅为缺失资源创建测试桩，结束后清理，不覆盖真实内核。资源测试覆盖页面/PWA 与内核解包；普通 Cargo 默认构建仍须先准备资源。

- 默认 Cargo 检查覆盖 core + cli；不要用 `cargo test --workspace`，它会额外构建桌面壳和 WebKit。
- 测试用临时目录、假内核、localhost 订阅；`spawn_server` 设置 `skip_extract: true`，注入隔离配置、运行目录与 `volatile_path`，不得读写生产目录或启动真实 TUN。网络验收用独立网络命名空间或专用机器。
- 修改 shell 跑 `shellcheck`，修改 workflow 跑 `actionlint`；前端约定见[检查与测试](frontend-rsbuild/README.md#检查与测试)。

[CI](.github/workflows/ci.yml) 和[发布 workflow](.github/workflows/build-release.yml) 仅手动触发，复用 [quality.yml](.github/workflows/quality.yml)；push、PR、tag 不自动检查或发版，日常使用本地脚本。

## 构建、发布与部署

```bash
./build.sh              # 前端、内核/规则、本机原生与 arm64 musl
./build.sh --no-arm64   # 仅本机原生
```

产物为 `target/release/miao-rust` 和 `target/aarch64-unknown-linux-musl/release/miao-rust`。交叉构建需 `zig`、`cargo-zigbuild` 和对应 Rust target；Arch 可安装 `sudo pacman -S zig cargo-zigbuild`，构建脚本会补装 arm64 target。规则只随 host 构建，arm64 只补内核。

只改前端时运行 `./scripts/build-frontend.sh`，再 `cargo build --locked --release`；内核变化先[重建 embedded](docs/kernel.md#构建与嵌入)。只跑 Cargo 会继续嵌入旧资源。

发布前完成检查并保持工作区干净：

1. `./release.sh vX.Y.Z` 更新版本、提交并推送 master 与 tag；推送含 workflow 的提交使用 SSH。
2. `./scripts/release-local.sh vX.Y.Z` 构建两架构 musl 产物、校验文件和源码清单，通过已登录的 `gh` 上传 Release。

发布脚本要求版本与 tag 一致、HEAD 指向 tag；`--dry-run` 只构建，`--skip-build` 复用 `dist/`。私有 Release 的下载与应用内升级限制见[平台说明](docs/platforms.md#升级与卸载)。

### Arch 生产实例

**本机 systemd miao 是出网依赖，普通开发不得停服或重启。** 不要裸跑 `target/release/miao-rust`、`cargo run -p miao-cli` 或 `cargo run -p miao-desktop`，它们会争用 TUN、Clash API 和面板端口。

程序为 `/usr/local/bin/miao`，配置为 `/etc/miao/config.yaml`，运行目录为 `/tmp/miao-sing-box`，面板为 `http://localhost:6161`。生产升级只走以下流程，内核有变时先重建 embedded；安装脚本会停服、替换、重启，期间短暂断网：

```bash
./scripts/build-frontend.sh
cargo build --locked --release
sudo cp /usr/local/bin/miao "/tmp/miao.$(date +%s).backup"
sudo bash install.sh ./target/release/miao-rust
```

只读排查用 `systemctl status miao`、`sudo pgrep -x miao` 或 `sudo ss -tlnp 'sport = :6161'`；不要用易误匹配的 `pgrep -f`。`/api/status` 的 `data.pid` 是内核子进程。卸载前备份配置，`remove.sh` 会删除 `/etc/miao`。

## Windows 开发

桌面源码和手动 workflow 保留，当前不发布 Windows 安装包。在 Arch 安装 `mingw-w64-gcc`、执行 `rustup target add x86_64-pc-windows-gnu` 并准备资源后，可运行 `cargo check -p miao-core --locked --target x86_64-pc-windows-gnu`。链接器缺失时设置 `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc`。

`bun scripts/prepare-test-assets.mjs` 可补齐缺失资源，但单独调用不自动清理测试桩。桌面编译检查用 `cargo check -p miao-desktop --locked`；Arch 需 WebKitGTK，不能用启动桌面程序代替检查。

Windows 真机构建需要 Rust MSVC、VS C++ 工具、Bun、PATH 中的 Go；在 Git Bash 的仓库根目录执行：

```bash
./scripts/build-frontend.sh
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh
cd desktop/src-tauri
bunx @tauri-apps/cli@2.11.4 build --bundles nsis --ci
```

重编、安装前从托盘退出。编译不替代真机 UAC、托盘、TUN、升级验收；维护时保留：

- 参数先校验再提权；`Global\` mutex（无权时退 `Local\`）防多开，已有实例不再 UAC；Wintun 由内核提供。
- 内核用 `CREATE_NEW_PROCESS_GROUP`，先 `CTRL_BREAK`，超时终止并清理 `sing-tun`；重载为停止/启动。
- 面板仅绑定 loopback，冲突可用随机端口；不增加 Tauri 远程 IPC，不编入 VPS/askpass、自替换升级或 OpenWrt 安装器，专属路由测试断言 404。
- 自启须显式勾选，使用最高权限的 `Miao` 登录任务和 `--minimized`，启动时修复失配路径。
- NSIS 安装/卸载拦截运行中的 `miao.exe`；升级 CLI 须核对自定义模板中的两处检查。

## 跨层维护约定

API 类型、mock 与浏览器约定集中在[前端文档](frontend-rsbuild/README.md)；业务事务集中在[状态文档](docs/runtime-state.md)；路径只能取自已解析的 `AppState`，见 [Profile](docs/profiles.md)。服务端 HTTP client 使用 `no_proxy()`；VPS 凭据走 stdin、主机密钥用 `accept-new`，Hysteria2 固定版本与官方 `hashes.txt` 一起校验。
