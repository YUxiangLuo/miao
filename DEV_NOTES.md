# 开发指南

Linux/OpenWrt CLI 与 Windows 桌面共用 `miao-core` 和 React 面板。Tauri 只承载本地 HTTP 页面、托盘和提权，不另建业务 API；Linux 保持单文件启动、TUN 接管流量的使用方式。

## 代码与文档入口

| 目录 | 职责与约定 |
| --- | --- |
| `crates/miao-core` | 配置、REST/MCP、内核管理；见[状态与事务](docs/runtime-state.md)、[Profile 路径](docs/profiles.md) |
| `crates/miao-cli` | Linux/OpenWrt 入口，产物 `miao-rust` |
| `desktop/src-tauri` | Windows Tauri 壳，非 workspace 默认成员 |
| `frontend-rsbuild` | 唯一面板；见[前端开发、设计与测试](frontend-rsbuild/README.md) |
| `scripts/sing-box` | 固定源码、工具链、裁剪补丁和 Go 回归；见[内核维护](docs/kernel.md) |
| `public` / `embedded` | 生成的前端和内核/规则资源，不入库，由 Rust 嵌入 |

用户用法见 [README](README.md)、[配置参考](docs/config.md)和[平台说明](docs/platforms.md)。版本与依赖以清单、lockfile 和 CI 为准，不在开发笔记中重复维护。

## 开发检查

命令在仓库根目录执行。完整构建需要 Bun、Go、Rust、Git、curl；Windows 和交叉构建另见下文。

```bash
./scripts/test-rust.sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
bun test scripts
```

`test-rust.sh` 会构建前端，仅为缺失资源创建 inert stub，并在结束时清理；不覆盖已有真实内核。直接运行 Cargo 前须先准备嵌入资源。默认测试只覆盖 core + cli，**不要用 `cargo test --workspace`**，它会额外构建桌面壳和 WebKit。

- 常规测试使用临时目录、假内核和 localhost 订阅；`spawn_server` 必须设置 `skip_extract: true` 并注入临时 `volatile_path`，不能读取或写入生产运行目录。
- 写路径测试必须注入隔离环境，不能启动真实 TUN。真实内核的网络验收使用独立网络命名空间或专用机器。
- 修改 shell 脚本运行 `shellcheck`；修改 workflow 运行 `actionlint`。前端检查命令和 mock 约定见[前端文档](frontend-rsbuild/README.md#检查与测试)。

## 构建、发布与部署

```bash
./build.sh  # 前端、内核/规则、本机原生 release → target/release/miao-rust
```

只改前端时，先 `./scripts/build-frontend.sh` 再 `cargo build --locked --release`；只跑 Cargo 会继续嵌入旧页面。内核变化先按[内核文档](docs/kernel.md#构建与嵌入)重建 `embedded/`，再构建 Miao。

[CI](.github/workflows/ci.yml) 在 master push 和 PR 上调用 [quality.yml](.github/workflows/quality.yml)：前端检查、三目标内核构建、Rust 检查与 Windows 桌面编译。[Build Release](.github/workflows/build-release.yml) 先跑质量检查，再构建 Linux musl 矩阵和 Windows NSIS；tag 触发时上传 Release，手动触发只保留 artifacts。发版入口为 `./release.sh vX.Y.Z`，各平台同号，发布产物以 CI 为准。推送含 workflow 的提交使用 SSH。

### Arch 生产实例

**本机 systemd miao 是出网依赖，普通开发不得停服或重启。** 不要裸跑 `target/release/miao-rust`、`cargo run -p miao-cli` 或 `cargo run -p miao-desktop`；它们会争用 TUN、Clash API 和面板端口。

| 项目 | 位置 |
| --- | --- |
| 程序 / 配置 | `/usr/local/bin/miao` / `/etc/miao/config.yaml` |
| 运行目录 / 面板 | `/tmp/miao-sing-box` / `http://localhost:6161` |

生产升级只走以下流程；内核有变时先重建 embedded。安装脚本会停服、替换并重启，期间短暂断网。

```bash
./scripts/build-frontend.sh
cargo build --locked --release
sudo cp /usr/local/bin/miao "/tmp/miao.$(date +%s).backup"
sudo bash install.sh ./target/release/miao-rust
```

只读排查用 `systemctl status miao`、`sudo pgrep -x miao` 或 `sudo ss -tlnp 'sport = :6161'`，不要用 `pgrep -f` 误匹配包装进程。`/api/status` 的 `data.pid` 是内核子进程。`remove.sh` 会删除 `/etc/miao`，卸载前备份配置。

## Windows 开发

Arch 上先安装 `mingw-w64-gcc` 并执行 `rustup target add x86_64-pc-windows-gnu`，准备资源后检查：

```bash
cargo check -p miao-core --locked --target x86_64-pc-windows-gnu
```

缺失资源可用 `bun scripts/prepare-test-assets.mjs` 补齐（单独调用不会自动清理）。链接器未找到时设置 `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc`。桌面壳由 Windows CI 执行 `cargo check -p miao-desktop --locked`；在 Arch 做同一检查需安装 WebKitGTK，不能用启动桌面程序代替编译检查。

Windows 真机构建需要 Rust MSVC、VS C++ 工具、Bun 和 PATH 中的 Go。在 Git Bash 中从仓库根目录执行：

```bash
bun run --cwd frontend-rsbuild build
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh
cargo build -p miao-desktop --locked --release
cd desktop/src-tauri
bunx @tauri-apps/cli@2.11.4 build --bundles nsis --ci
```

重编/安装前从托盘退出，运行中的 exe 会被锁定。编译通过不替代真机 UAC、托盘、TUN 分流和升级验收。维护桌面壳时保留以下约定：

- 参数校验先于提权；用 `Global\` mutex（无权时退 `Local\`）防止多开，已有实例不再次 UAC。Wintun 由内核提供，不额外分发 DLL。
- 内核使用 `CREATE_NEW_PROCESS_GROUP`，先 `CTRL_BREAK`，超时再终止并清理 `sing-tun`；重载以停止/启动实现。生命周期与 watchdog 规则见[状态文档](docs/runtime-state.md#内核生命周期与控制权)。
- 面板绑定 `127.0.0.1`，端口冲突时可退到随机端口；不增加 Tauri 远程 IPC。Windows 不编入 VPS 部署、askpass、自替换升级或 OpenWrt 安装器，专属路由测试断言 404。
- 自启使用 `Miao` 登录任务、最高权限和 `--minimized`，用户须显式勾选；每次启动回读任务路径，升级后失配则重注册。
- NSIS 安装/卸载前拦截仍在运行的 `miao.exe`；升级 Tauri CLI 时核对自定义模板中的两处检查。

## 跨层维护约定

- Rust serde models 是 Miao API 类型的唯一来源；生成方式、Clash 类型和测试工厂见[前端文档](frontend-rsbuild/README.md#api-类型与请求)。
- REST 与 MCP 共用业务服务；配置编辑、节点切换、回滚、订阅代次和 readiness 必须遵守[状态与事务](docs/runtime-state.md)，不要在 handler 复制逻辑。
- 配置、缓存、偏好和日志路径只从已解析的 `AppState` 获取；CLI/桌面共用参数解析器，SDK 不读取宿主 argv，详见 [Profile 规则](docs/profiles.md)。
- 服务端 HTTP client 使用 `no_proxy()`。VPS Hysteria2 版本与 `hashes.txt` 一起校验，SSH 凭据走 stdin，主机密钥使用 `accept-new`；网络等待不放进配置锁。
