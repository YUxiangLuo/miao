# 开发调试笔记

本机（Arch）开发 miao 的实战经验，面向后续开发者/agent。命令以仓库根目录为基准。

## 日常开发命令

```bash
# 前端(vite 开发服务器,/api 代理到 6161,支持 ws)
bun run --cwd frontend dev          # 5173 端口;后端端口改过时用 MIAO_API=http://localhost:7000 覆盖
bun run --cwd frontend test         # vitest
bun run --cwd frontend lint         # eslint --max-warnings=0

# 后端
cargo test --locked --all-targets   # 单测(纯函数/handler 错误路径/读路径),与 CI 一致
cargo fmt --all -- --check          # CI 门禁之一,提交前必跑
cargo clippy --locked --all-targets --all-features -- -D warnings   # 同上
# CI 还跑 cargo audit / bun audit(依赖安全审计),发布前值得本地过一遍

# 全量构建(前端 + sing-box 源码 + geo 规则 + release 二进制)
./build.sh

# 发版(bump Cargo.toml + tag,CI 自动构建 release)
./release.sh vX.Y.Z

# CI 监控
gh run list --limit 5 && gh run watch <id> --exit-status
```

**前端改动只跑 `cargo build` 不会生效**:`include_str!` 嵌入的是 `public/index.html`(上一次 vite 构建的产物),它不变 cargo 照样编译通过,装进服务的是旧页面。改前端后必须 `./scripts/build-frontend.sh`(或全量 `./build.sh`)再 `cargo build`。

**首次克隆的前置步骤**:`embedded/` 下的 sing-box 二进制与 3 个 srs 规则集、GeoIP 城市数据库(`geoip-city.mmdb`,地图模式用)不入库(.gitignore),而 Rust 代码用 `include_bytes!` 引用它们——fresh clone 直接 `cargo build`/`cargo test` 会因缺文件编译失败。先跑一次 `./scripts/build-embedded.sh`(编译 sing-box 源码 + 下载 geo 规则与 mmdb,耗时较长)或 `./build.sh`。CI 的 quality 流水线是用空壳 stub 文件绕过,别学。

## 本机调试环境

- 常驻运行的是 **systemd 服务**(`/usr/local/bin/miao`,`WorkingDirectory=/etc/miao`,`Restart=on-failure`)。配置在 `/etc/miao/config.yaml`，运行时文件在 `/tmp/miao-sing-box`，面板 `http://localhost:6161`
- 调 dev 二进制 / 装新构建的推荐流程：

```bash
cargo build --locked --release        # 前端有改动先 ./scripts/build-frontend.sh
sudo systemctl stop miao              # 先停服务,否则 6161 被占
sudo nohup ./target/release/miao-rust > /tmp/miao-dev.log 2>&1 &
# 调完让服务直接跑新构建(install.sh 离线路径 = 安装即升级):
sudo bash install.sh ./target/release/miao-rust
```

- **kill 进程别用 `pgrep -f 'target/release/miao-rust'`**：它会同时匹配 `sudo nohup ...` 包装进程，`head -1` 拿到包装进程，kill 它杀不掉真正的子进程（实测坑过：端口仍被占，`install.sh` 报"端口 6161 已被占用")。用进程名精确匹配：`sudo pgrep -x miao-rust`(systemd 服务是 `sudo pgrep -x miao`)；或 `sudo ss -tlnp 'sport = :6161'` 直接找监听者
- 后端跑在 root 下，**普通用户 pgrep 看不到**
- **每次启动都会删除并重释放内嵌文件**（sing-box + 3 个 srs）到 `/tmp/miao-sing-box`,rebuild 后重启一定用上新内核，这些文件的时间戳每次都会变，别困惑;`cache.db`/`config.json.cache` 有意保留
- `api/status` 里的 `data.pid` 是 **sing-box 子进程**的 PID，不是后端本身——误杀它只会让面板显示"已停止"但 6161 仍被后端占用（后端没有 supervisor，不会自动拉起 sing-box)。要停就停后端主进程
- 出口验证手法：把 selector 切到节点 A（面板或 `PUT /api/clash/proxies/proxy`)，再对有进程规则的程序跑 `curl ifconfig.me`，与 `wget -qO- ifconfig.me/ip`（无规则对照）比 IP，即可确认进程级分流是否生效

## 网络与代理的鸡生蛋问题

这台机器的出网流量被 miao 自己的 TUN 接管。**后端一停，GitHub 就访问不了**。相关推论：

- `install.sh` 的在线下载路径没法在本机测；用离线路径：`sudo bash install.sh ./target/release/miao-rust`
- 同样道理，改网络栈/sing-box 配置的改动，先想好后端停了之后怎么验证
- `remove.sh` 会删 `/etc/miao`（含用户配置）!**实测前先备份**:`sudo cp -a /etc/miao /tmp/miao-config-backup`
- git push：直接用 SSH(`origin` 就是 `git@github.com:...`,`~/.ssh/id_ed25519` 已恢复可用,且 **HTTPS + gh 凭据推不动 workflow 文件改动**——token 缺 `workflow` scope;SSH 无此限制)。若走 HTTPS 则：`git -c credential.helper='!gh auth git-credential' push https://github.com/YUxiangLuo/miao.git master`,注意 URL 推送不更新 tracking ref,需要时补 `git update-ref refs/remotes/origin/master master`

## 前端调试（agent-browser)

- 常用：`agent-browser open <url>` / `eval '<js>'` / `screenshot [--full] <path>`;`set viewport <w> <h>` 调窗口尺寸（测布局/响应式必备）
- `eval` 的 JS 上下文**跨调用保留**：同名 `const`/`let` 再声明直接报错，代码用 IIFE 包起来：`(() => { ... })()`
- **React 受控组件赋值**要用原生 setter + 事件，否则 onChange 不触发（input 发 `input` 事件；select 发 `change` 事件，原型描述符在 `HTMLSelectElement` 上）:

```js
const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set
setter.call(input, '新值'); input.dispatchEvent(new Event('input', { bubbles: true }))
```

- 首页是固定高度布局：`document.body.scrollHeight == innerHeight`，页面不滚动，**左右列各自内部滚动**(`.left-column`/`.right-column`)，别被"截断"误导
- 布局恒定三原则（改布局前必读）:
  - 活跃链接区（`.home-connections`）恒高 240px 且**始终渲染**（无活跃显示占位文案）;`.content-grid` 用 `flex: 1 1 auto` 占满剩余空间。两者都不随活跃链接出现/消失变化——别再写"无连接时不渲染"或 `:last-child` 变高这类逻辑
  - 每列最后一张卡片 `flex-grow: 1` 填满列高，保证左右列卡片底边对齐
  - 左列节点卡片恒等于列高（`min-height: 326px` 保底，列高不足时退回整列滚动）：头部与当前节点横幅固定，仅 `.proxy-grid-wrap` 内部滚动
  - 移动端（≤840px）由 JS 的 `isDesktop` 门控不渲染活跃链接区，不要再加 CSS `display:none` 双保险
- 链接统计面板入口：状态卡的流量数字（`.traffic-chip`)

## 写测试的坑（都已踩过）

**前端（vitest + testing-library)**:

- `<details>` 折叠时子元素**仍在 DOM 中**——断言折叠要查 `open` 属性，不是元素不存在
- 规则添加表单在弹窗（RuleModal）里，**关闭时完全不在 DOM 中**；打开后字段标签会同时出现在列表行 chip 和下拉 `<option>` 里，用 `getAllByText` 或精确化选择器
- 节点密码校验 ≥8 位，测试数据别用 `pass123`;ss 的 2022-blake3 系列 cipher 要求密码是 base64 key,sing-box 会拒
- `user.click(input)` 后 `user.paste(text)` 模拟粘贴
- App 集成测试要 mock 全端点：`/api/status|subs|nodes|rules|version`、`/api/clash/proxies`、`/api/clash/proxies/*/delay`;WebSocket 失败会自行重连，组件卸载时清理定时器，不用管

**后端（cargo test)**:

- 只测纯函数、校验失败、读路径和**不触网**的 handler 路径（如已存在直接返回）。成功的写路径会走 `apply_config_change` → 真实生成配置 + 起 sing-box，别在单测里碰
- handler 测试套路：`test_support::test_app(Config{..})` + `oneshot(json_request(...))`;`Config` 实现了 `Default`，新测试可用 `..Default::default()` 少写字面量
- 新增配置字段会波及所有既有测试里的 `Config{..}` 字面量，sed 批量删/加即可

## 后端架构速记

- 配置变更链路（增删节点/订阅/规则都走这条）:`config_update` 锁 → 克隆配置 → 改 → `apply_config_change`（原子写 config.yaml → 生成 sing-box 配置 → `sing-box check` 校验 → 热重启，失败回滚）
- 面板不直接碰 sing-box，控制面是 Clash API(`127.0.0.1:6262`)：切节点、测延迟、连接统计都走它
- 自定义规则（`custom_rules`）插入在 sniff/hijack-dns 之后、内置分流规则**之前**，用户规则优先；全局模式下内置分流被裁掉，自定义规则依然生效
- 规则可指向具体节点 tag（面板下拉或手写）;`build_sing_box_config` 生成时会跳过引用不存在节点的规则，记入 `state.skipped_rules`——状态接口并入 `warning`（面板 toast)，规则列表按 raw 匹配标记 `skipped`（警示 icon，提示删除后重配）。节点回来了规则自动恢复生效
- 添加规则时 `Validator::custom_rule` 会拿 `known_rule_targets`（手动节点 + 运行时配置里的订阅节点 tag）做存在性校验，但那只是友好报错；生成时跳过才是兜底
- `route_mode`（分流/全局）是会话级状态，不进配置文件
- **地图模式**:`GET /api/map/overview` 聚合 Clash connections 并按目的 IP 本地定位（内嵌 `geoip-city.mmdb`,maxminddb 读,LRU 缓存 4096 条);本机真实出口靠内置直连规则（`cip.cc`/`myip.ipip.net`，全局模式也保留）+ IP 回显服务探测,`config.yaml` 的 `location: "lat,lng"` 可手动覆盖;代理节点位置 = selector 当前节点 server 解析,机场占位 IP(如 127.127.127.5)时回退 AliDNS/Cloudflare DoH 重查。入口在状态卡「地图模式」按钮,前端 Leaflet + turf 大圆航线,单测里 leaflet 整体 mock,注意 mock 的 `layerGroup.addTo` 必须返回自身(组件存返回值进 ref)
- **最后选择的节点**持久化在 `.last_proxy`(JSON `{group, name}`)，路径按平台分：OpenWrt/非 systemd → `/tmp/miao-sing-box/`（避免写 flash)；普通 systemd Linux → **工作目录**（本机 unit 是 `/etc/miao`)。恢复时机：sing-box 启动后约 1s，经 Clash API PUT 选中；节点不在当前列表则跳过（**文件不自清**，下次面板切换才覆盖）。只有走 `/api/last-proxy` 的调用（即面板切换）会更新它；直接调 Clash API 切节点不会

## 脚本

- `install.sh` / `remove.sh` 提交前跑 `shellcheck`
- `install.sh` 顺序：本地路径先停 miao.service → `ss` 预检 6161 端口占用 → 占用中则报错退出。装新构建前确保 dev 二进制已停
- `remove.sh` 的确认提示刻意从 `/dev/tty` 读：`curl | sudo bash` 管道场景也能提示；**别再给那个 read 加重定向**，`read -p` 的提示语走 stderr，曾被 `2>/dev/null` 吞掉导致"假卡住"
