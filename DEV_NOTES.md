# 开发调试笔记

开发机两台：**Arch**（Linux 日常开发 + 常驻生产 miao 实例）与 **Windows**（桌面版真机，全量构建链）。**master 单分支**，Linux CLI 与 Windows 桌面版同出。命令以仓库根目录为基准，未注明时在 Arch 上执行。

## 要守住的哲学

Linux 版是：下载一个文件，`sudo` 跑，浏览器打开，TUN 接管流量，面板就是产品。Windows 只换外壳，不换产品。

| 守住 | 不要做成 |
| --- | --- |
| 同一套 `miao-core` + 同一块 React 面板 | 用 Tauri invoke 重写 API，或再做一份前端 |
| 没配置就进引导页 | 安装向导、强制落盘 |
| TUN 透明代理 | 系统代理（WinINET）当主路径 |
| Tauri 只当自带浏览器，打开 `http://127.0.0.1:<port>` | Linux / OpenWrt 也改成 Tauri |
| 一次 UAC 对标一次 `sudo`；自启须用户显式勾选（任务计划，不是服务） | 服务模式 / 默认免 UAC |
| Wintun 已在 sing-box 里；内核跟默认分支走 | 旁边再塞一份 `wintun.dll` |
| Windows 构建不编 VPS、不编换进程升级 | 把 Linux 的 `exec` 热更搬过去覆盖正在跑的 exe |

**Arch 上正在跑的 systemd miao 不要停、不要重启**——出网被它的 TUN 管着，一停 GitHub 就没了。

## 仓库结构

```
crates/miao-core      库：面板、配置事务、启停内核
crates/miao-cli       Linux/OpenWrt 入口，二进制名 miao-rust
desktop/src-tauri     Tauri 2 壳（workspace 成员但不是 default-member）
frontend-rsbuild/     唯一一份面板（React 19 + TypeScript strict + Rsbuild）
public/               rsbuild 构建产物（gitignore），被 include_str! 嵌进 core
embedded/             sing-box + srs，不入库
```

`cargo test` / `cargo clippy`（不加 `--workspace`）只打 core + cli；**不要 `cargo test --workspace`**（会去编桌面壳、拉 webkit）。

## 前端栈与跨层约定

- **TypeScript 钉 7.0.2**：lint 由 rslint 承担。升级前先跑 `lint + typecheck + test` 三件套验证。
- **前后端类型单一来源**：`crates/miao-core/src/models/*.rs` 的 serde 结构体是 canonical schema，`frontend-rsbuild/src/types/api.ts` 由 ts-rs 生成，**禁止手改**。改 API models 后运行 `./scripts/generate-api-types.sh`；普通 Rust 测试会对比生成结果，文件过期直接失败。`types/clash.ts` 无 Miao 后端 schema（Clash API 反代），按前端实际消费面维护。

## 日常命令

```bash
# 前端
bun run --cwd frontend-rsbuild dev / test / lint / typecheck
./scripts/generate-api-types.sh  # 修改 Rust API models 后更新 TS binding

# 默认成员门禁（与 CI 一致）
./scripts/test-rust.sh  # fresh clone 也可直接运行；只用临时 inert 内核，不启动 TUN
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings

# Windows 静态门禁（不启动任何代理）
cargo check -p miao-core --target x86_64-pc-windows-gnu
cargo check -p miao-desktop          # 需本机有 webkit2gtk；不要 cargo run

# 内核（只写 embedded/）
./scripts/build-embedded.sh                         # Linux 本机架构
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh

# 前端改完必须重新嵌入（只 cargo build 还是旧页面）
./scripts/build-frontend.sh

./build.sh                 # 本机全量构建
./release.sh vX.Y.Z        # 发版：bump 版本 + tag，全平台同号
```

**禁止**（会破坏本机生产实例）：

```bash
sudo systemctl stop miao
sudo ./target/release/miao-rust   # 裸跑会与生产实例抢 TUN 和 6161 端口
cargo run -p miao-desktop
cargo run -p miao-cli
# 任何会往 /tmp/miao-sing-box 抽内核的测试（spawn_server 测试必须 skip_extract: true，
# 且必须注入 volatile_path 临时路径，否则读写的是真实运行时目录）
```

**合法升级本机生产实例的唯一姿势**：`./scripts/build-frontend.sh && cargo build --locked --release && sudo bash install.sh ./target/release/miao-rust`。install.sh 会停服→换二进制→重启，断网窗口约 2 秒；升级前先 `sudo cp /usr/local/bin/miao /tmp/miao.$(date +%s).backup` 留回滚。注意只跑 `cargo build` 不重嵌前端，部署的还是旧页面。

**PWA**：`frontend-rsbuild/public/` 的 manifest/sw.js/图标经 rsbuild 拷进 `public/` 再嵌进二进制——新增静态资源必须同时在 `router.rs` 注册路由。SW 只是 Chrome 安装门槛的门票：只给导航请求做 network-first 兜底，**永远别缓存 `/api`**。

**fresh clone**：跑 Rust 测试直接用 `./scripts/test-rust.sh`，它会构建前端、只为缺失资源临时创建 inert stub，并在测试结束时清理；不会启动代理或 TUN，也不会覆盖已有真实内核。构建可运行产物仍用 `./scripts/build-embedded.sh` 或 `./build.sh`。Windows 交叉 check 需要真实或显式准备的 `embedded/sing-box-windows-amd64.exe`。

## 在 Arch 上怎么「做」Windows

验收口径：测试绿、clippy 过、`windows-gnu` check 过、CI 的 `windows-latest` **编过**桌面壳。启动/UAC/托盘/TUN 分流的行为验收到 Windows 真机上做。

```bash
sudo pacman -S --needed mingw-w64-gcc
rustup target add x86_64-pc-windows-gnu
# 链接器找不到时：export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
```

Windows 构建里故意拿掉的能力（管理员进程里不该有；Linux CLI 保留）：

- `POST /api/upgrade` 换 exe 自升级
- `POST /api/vps/deploy` 和 askpass
- OpenWrt apk/opkg 安装器
- Tauri 对 `http://127.0.0.1:*` 的远程 IPC

## 在 Windows 真机上构建

工具链：Rust（msvc）+ VS 2022 C++、Go（`%LOCALAPPDATA%\Programs\go`）、Bun。Tauri CLI 用固定版本 `bunx` 临时执行，不做 npm 全局安装。

```powershell
bun run --cwd frontend-rsbuild build  # 改了前端才跑
# Git Bash（PATH 先带上 Go）：
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh
cargo build -p miao-desktop --locked --release
# desktop/src-tauri 下：
bunx @tauri-apps/cli@2.11.4 build --bundles nsis --ci
```

- **重编前先从托盘退出 miao**：运行中的 exe 被锁定，链接报 `os error 5`
- 运行 `miao.exe` 会弹 UAC 并用 TUN 接管本机流量，调试时别在还要出网的会话里乱杀进程
- 本地产物仅自测，发布以 CI 产物为准

## Arch 的 Linux 实例（生产依赖，不是调试对象）

systemd 服务：`/usr/local/bin/miao`，配置 `/etc/miao/config.yaml`，运行时 `/tmp/miao-sing-box`，面板 `http://localhost:6161`。别拿它调试、别随手重启；升级走前文「合法升级」一条。

- 查进程用 `sudo pgrep -x miao` 或 `sudo ss -tlnp 'sport = :6161'`（进程名来自安装路径 `/usr/local/bin/miao`，不是构建产物名 `miao-rust`；`pgrep -f` 会误匹配 nohup 包装进程；root 进程普通用户看不到）。服务整体状态直接 `systemctl status miao`
- `api/status` 的 `data.pid` 是 **sing-box 子进程**，不是后端
- 每次启动重释放内嵌文件到运行时目录；`cache.db` / `config.json.cache` 有意保留
- `remove.sh` 会删 `/etc/miao`，动之前先备份
- git push 用 SSH（HTTPS + gh token 缺 `workflow` scope，推不动 workflow 文件）

## Windows 路径与行为（写代码时对照）

| 项 | 实现 |
| --- | --- |
| 配置/日志/上次节点 | `%LOCALAPPDATA%\io.github.yuxiangluo.miao\`（config.yaml、miao.log、.last_proxy） |
| 内核运行时 | `%TEMP%\miao-sing-box` |
| 面板绑定 | `127.0.0.1` |
| 单实例 | 命名 mutex 优先 `Global\`（跨会话防双开抢 TUN），无权退 `Local\`；未提权先 `OpenMutexW`，已有实例不再 UAC |
| 停核 | `CREATE_NEW_PROCESS_GROUP` + `CTRL_BREAK`；超时再杀并只清 `sing-tun` |
| 提权 | 确认无实例后再 `ShellExecuteW runas`；取消则 MessageBox |
| 端口 | `port_fallback`：6161 被占时改绑随机端口 |
| 内核看门狗 | 崩溃自动拉起（2s 巡检、1–16s 退避、最多 5 次），放弃时写 `config_warning`；有意启停先递增 `sing_generation` |
| 日志轮转 | `miao.log` 超 8 MB 启动改名 `.old`（单份滚动） |
| 托盘 | 单击唤出、双击唤出/收回；菜单：显示窗口/打开日志/开机自启/退出 |
| 开机自启 | 任务计划 `Miao`（ONLOGON + HIGHEST + `--minimized`），勾选状态即任务存在性、不落配置，切换后回读校验；每次启动校验任务指向的 exe 路径与当前进程一致，不一致（升级/迁移后旧任务残留拉起旧版本）自动用当前路径重注册 |
| NSIS | 自定义模板 `nsis/installer.nsi`：安装/卸载前查 miao.exe 在跑则拦（提权进程杀不掉，只能拦）。升 tauri-cli 版本要重套这两处 |

TUN JSON：`auto_route` + `strict_route`，`interface_name` 仍是 `sing-tun`。`cfg(target_os = "linux")` 才写 `auto_redirect`。

## 写测试

**后端**：只测纯函数、校验失败、读路径、不触网的 handler。成功写路径会起真实 sing-box，单测别碰。`spawn_server` 必须 `skip_extract: true` + 注入 `volatile_path` 临时路径。Windows 专属路由测试用 `#[cfg(windows)]` 断言 `/api/upgrade`、`/api/vps/deploy` 是 404。

**前端**：App 集成测试 mock `/api/status|subs|nodes|rules|version` 和 Clash 端点。`setupTests.ts` 全局 stub 了 `matchMedia`（恒 false=深色）与 `localStorage`（Node ≥22 内建版遮蔽 jsdom，返回 undefined，故用内存 stub）。**构造 API/Clash 类型的 mock 一律用 `src/testFixtures.ts` 的工厂**（`statusMock`/`connectionMock`/`connectionGroupMock`/`ruleMock`）：后端加必填字段时在工厂补默认值，测试只覆盖差异字段，不要手写散装对象。

## 设计 token 约定

样式唯一来源是 `frontend-rsbuild/src/styles/tokens.css`（九段：主题色/派生色/音阶/动效/层级/透明度/描边/控件几何/组件几何）。写样式先找 token，没有再新增；**禁止组件里出现硬编码颜色/尺寸/时长**。例外（需在注释说明）：`@media` 断点（CSS 不支持 var，JS 镜像在 `src/tokens.ts`）、≤3px 光学补偿 padding、onboarding 门面散值、`siteIcons.ts` 第三方品牌色。

- **双主题**：仅段 1 分主题——`:root` 深色（默认）+ `:root[data-theme="light"]` 亮色。其余段主题无关。语义色的 alpha 面/描边一律 `color-mix(in srgb, var(--x) N%, transparent)` 现算，随主题自动派生，不要写死 rgba。
- **色彩语义**：紫（`--accent` 族）= 交互与选中（品牌色，与 logo 同族）+ 上传速率；蓝（`--info` 族）= 代理路径（出口 chip、规则字段）+ 下载速率；绿专属直连/红拦截/琥珀警告——彩色不再跨维度复用（曾因上传绿与直连绿冲突导致链接统计页读混乱）。选中态用 `--accent-tint`，不要用 info 蓝。
- **圆角角色档**：`--r-sm` 控件 / `--r-md` 内容块 / `--r-lg` 容器 / `--r-xl` 浮层 / `--r-pill` 徽章；同心规则：外层恒比内层大一级。
- **主题切换**：只有显式 dark/light 两态，默认 dark，不跟随系统；`hooks/useTheme.ts` + `index.html` 内联引导脚本防 FOUC。主题键 `miao-theme`，开关在顶栏（Sun/Moon）。新增主题 = 复制段 1 改值。
- **JS 侧常量**（图标尺寸、断点、主题键）统一 `src/tokens.ts`，与 CSS 注释互指、双向同步。
- JS 消费 token：lucide 用 `ICON.xs/sm/md/lg`，logo 用 `LOGO_SIZE`，不要写字面量。

## 配置与内核管线（后端核心）

变更链路：订阅增删/刷新先在锁外拉取，按 `sub_refresh_generation` 淘汰旧请求，再持 `config_update` 锁合并当前设置、生成候选、`sing-box check`、激活和提交。显式停服取消在途订阅刷新；校验子进程限时 10 秒。面板读取 Clash API 反代；节点切换统一走 `POST /api/proxy/switch`，与 MCP 共用串行切换/持久化服务，并使旧恢复任务失效。

生成配置无条件带 `route.find_process: true`（builder.rs）：面板「链接统计」每行副标题的进程名依赖 Clash API 的 `processPath`，而 sing-box 只在有进程类规则或此开关下才跑进程搜索器——删掉它，没有进程规则的用户面板就没有进程列数据。

落盘分层（详见 docs/config.md）：稳定层 `config.yaml`（port/subs/nodes/custom_rules/mcp + 手写启动默认值）、易变层 `volatile.yaml`（有效 node_select/max_multiplier/route_mode——unix 在 tmpfs，Windows 持久）、选择偏好 `.node_select` / `.max_multiplier` / `.last_proxy`（普通 systemd Linux 与 Windows 跨重启，OpenWrt 仍在 tmpfs）、状态层（可删）。显式选择策略和最高倍率优先于 volatile/config 默认；`AppState::node_select_preference` 保存 requested strategy，`config.node_select` 保存 effective strategy，启动期地区筛空的 manual 不覆盖前者，刷新/配置事务必须重新 overlay requested。配置分层和选择偏好均原子写并跳过未变。倍率从完整节点池的当前显示名动态收集，未标倍率按 1x、无效显式倍率 fail-closed；只在 requested node_select 为 fastest_* 时过滤 urltest 候选，真实订阅/手动 outbound 和 manual selector 成员始终完整保留。地区筛选和倍率过滤必须携带绑定前的当前显示元数据，不能从可能保留历史名称的稳定 tag 重新解析。

失败回滚去网络化：先快照 `config.json` 字节，回滚按 **内存快照 → `config.json.cache` → 本地节点快照/手动节点** 分层；内核已死时先 `sing-box check` 再启动；空 cache 拒绝恢复。持锁回滚不触网；本地材料不足时保留可用运行态并报错，由显式刷新或启动后台恢复负责网络。

订阅刷新只有一条管线 `refresh_subscriptions`，策略 `RefreshPolicy`：`Manual`（用户在场，失败即报）/ `ManualInApply`（事务内，node_select 随外层事务提交）/ `Startup`（全失败保留运行中的缓存）。节点集来源 `SubSource`：本地语义变更用 `sub-nodes.json` 快照零网络重建，增删订阅/手动刷新/启动才真拉取。失败订阅按来源保留最近成功节点，成功空列表覆盖该来源；缓存节点不计入新鲜拉取健康度。快照在内存中共享不可变读模型，按提交替换。快照缺失的本地变更不退化为网络请求：纯手动运行态可本地重建，无法证明订阅材料完整则提示先刷新。

启动两条路：`config.json.cache` 存在且过 `sing-box check` → 秒开 + `Startup` 策略后台刷新（拉取阶段不持锁）；否则同步拉取。自升级健康点在内嵌文件释放成功且预期数据面 ready 后；空配置无需数据面，用户明确停服也视为运行状态已稳定。

依赖与供应链：YAML 用 `yaml_serde`（serde_yaml 已归档）；`AppState.http_client` 显式 `no_proxy()`（本进程自己就是代理）；VPS 部署与后台刷新都不在 `config_update` 锁内做网络等待；VPS 的 Hysteria2 钉版 + `hashes.txt` 校验（升级靠人工 bump `HYSTERIA_VERSION`），凭据经 stdin 注入不进 argv，SSH `accept-new`（TOFU）。

## MCP 端点

`POST /mcp`：MCP 无状态 JSON-RPC，配置 `mcp: true` 开启（默认关，关闭时 404）。工具覆盖面板的状态、版本、订阅、手动节点、规则、连接、模式、MCP 开关、VPS 与升级能力；浏览器本地主题/PWA 不属于服务端工具。写操作必须复用现有 handler/service，禁止复制配置事务。节点模型保持平铺，selector 永不暴露；`switch_node` 走 Clash PUT + `save_last_proxy`（与面板同路径）。破坏性工具必须带 `destructiveHint`、明确后果，并在执行前校验 `confirm: true`。测试优先覆盖目录/文案契约、纯分发、参数校验、确认闸和无网络读路径；不要在单测中实际升级、SSH、拉订阅或启动 TUN。

## CI

push/PR 跑 `ci.yml`：Frontend quality（install → audit → lint → **typecheck** → test → build）→ Rust quality（default-members + windows-gnu check）→ Windows 健康检查（`cargo test -p miao-core` + `cargo check -p miao-desktop`，内核用 stub，**不跑 exe、不开 TUN**）。

打 `v*` tag 或手动跑 **Build Release**：quality → frontend → 并行编 Linux musl 矩阵（zigbuild）与 Windows 桌面（真内核 + NSIS）→ tag 触发时产物传 GitHub Release，手动跑只留 artifacts。发布产物以 CI 为准。

## 脚本

- `install.sh` / `remove.sh`：提交前 `shellcheck`
- `build-embedded.sh` 的 `MIAO_TARGET=windows-amd64` 也会编 host 规则编译器 `sing-box-host`，不要拿它去抽本机正在跑的实例
- `SING_BOX_REF`、`SING_GEOIP_REF`、`DIRECT_RULES_REF` 接受分支/tag/完整 sha；本地默认跟上游分支，release CI 在单独 job 中把三者解析为一次快照并供所有平台共用，版本清单随 Release 发布

## 前端调试（agent-browser）

- `agent-browser open <url>` / `eval '<js>'` / `screenshot [--full] <path>`；`set viewport <w> <h>`
- `eval` 跨调用保留 JS 上下文，同名 `const` 会炸，用 IIFE
- React 受控组件要走原生 setter + `input`/`change` 事件
- Arch 的生产实例面板可用来看共享 UI，不能用来验 UAC / WebView2 / WinTun

## 面板读取与大列表

状态/代理/连接按 3 秒轮询；订阅、手动节点、规则按 `data_revision` 变化刷新，30 秒轮询兜底。首次读取由同一轮询器负责；资源请求统一限时、取消和代次检查，晚到结果不能覆盖新状态。测速批次在清空或卸载时取消，自动历史与手动测量按时间比较。

连接明细使用可视区虚拟列表，行高与间距分别由 `CONNECTION_ROW_HEIGHT` / `CONNECTION_ROW_GAP` 镜像 CSS token；更改几何尺寸须同步。FLIP 只测量可视行且仅在顺序变化时触发。规则活跃检测先为连接建立签名索引，避免规则数乘连接数的重复解析。
