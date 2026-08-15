# 开发调试笔记

本机（Arch）开发 miao 的实战经验，面向后续开发者/ agent。命令以仓库根目录为基准。

## 日常开发命令

```bash
# 前端(vite 开发服务器,/api 代理到 6161,支持 ws)
bun run --cwd frontend dev          # 5173 端口
bun run --cwd frontend test         # vitest
bun run --cwd frontend lint         # eslint --max-warnings=0

# 后端
cargo test --locked                 # 单测(纯函数/handler 错误路径/读路径)
cargo fmt --all -- --check          # CI 门禁之一,提交前必跑
cargo clippy --locked --all-targets --all-features -- -D warnings   # 同上

# 全量构建(前端 + sing-box 源码 + geo 规则 + release 二进制)
./build.sh

# 发版(bump Cargo.toml + tag,CI 自动构建 release)
./release.sh vX.Y.Z

# CI 监控
gh run list --limit 5 && gh run watch <id> --exit-status
```

## 本机调试环境

- 开发后端：`sudo ./target/release/miao-rust`，配置在 `/etc/miao/config.yaml`，运行时在 `/tmp/miao-sing-box`，面板 `http://localhost:6161`
- **重启后端是安全的**(SIGTERM 优雅停 sing-box；配置实时落盘）:

```bash
cargo build --locked --release
sudo kill -TERM $(sudo pgrep -f 'target/release/miao-rust' | head -1)
sleep 2 && sudo nohup ./target/release/miao-rust > /tmp/miao-dev.log 2>&1 &
```

- 后端跑在 root 下，**普通用户 pgrep 看不到**。找监听者用 `sudo ss -tlnp 'sport = :6161'`
- **每次启动都会删除并重释放内嵌文件**（sing-box + 3 个 srs）到 `/tmp/miao-sing-box`，所以 rebuild 后重启一定用上新内核，目录里这些文件的时间戳每次都会变，别困惑；`cache.db`/`config.json.cache`/`.last_proxy` 有意保留
- `api/status` 里的 `data.pid` 是 **sing-box 子进程**的 PID，不是后端本身——误杀它只会让面板显示"已停止"但 6161 仍被后端占用（后端没有 supervisor，不会自动拉起 sing-box)。要停就停后端主进程

## 网络与代理的鸡生蛋问题

这台机器的出网流量被 miao 自己的 TUN 接管。**后端一停，GitHub 就访问不了**。相关推论：

- `install.sh` 的在线下载路径没法在本机测；用离线路径：`sudo bash install.sh ./target/release/miao-rust`
- 同样道理，改网络栈/sing-box 配置的改动，先想好后端停了之后怎么验证
- `remove.sh` 会删 `/etc/miao`(含用户配置）!**实测前先备份**:`sudo cp -a /etc/miao /tmp/miao-config-backup`

## 前端调试(agent-browser)

- 常用：`agent-browser open <url>` / `eval '<js>'` / `screenshot [--full] <path>`
- **React 受控组件赋值**要用原生 setter + input 事件，否则 onChange 不触发：

```js
const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set
setter.call(input, '新值'); input.dispatchEvent(new Event('input', { bubbles: true }))
```

- 首页是固定高度布局：`document.body.scrollHeight == innerHeight`，页面不滚动，**左右列各自内部滚动**(`.left-column`/`.right-column`)，别被"截断"误导
- 链接统计面板入口：状态卡的流量数字(`.traffic-chip`)

## 写测试的坑(都已踩过)

**前端(vitest + testing-library)**:

- `<details>` 折叠时子元素**仍在 DOM 中**——断言折叠要查 `open` 属性，不是元素不存在
- 规则添加表单在弹窗（RuleModal）里，**关闭时完全不在 DOM 中**；打开后字段标签会同时出现在列表行 chip 和下拉 `<option>` 里，用 `getAllByText` 或精确化选择器
- 节点密码校验 ≥8 位，测试数据别用 `pass123`;ss 的 2022-blake3 系列 cipher 要求密码是 base64 key,sing-box 会拒
- `user.click(input)` 后 `user.paste(text)` 模拟粘贴
- App 集成测试要 mock 全端点：`/api/status|subs|nodes|rules|version`、`/api/clash/proxies`、`/api/clash/proxies/*/delay`;WebSocket 失败会自行重连，组件卸载时清理定时器，不用管

**后端(cargo test)**:

- 只测纯函数、校验失败、读路径和**不触网**的 handler 路径（如已存在直接返回）。成功的写路径会走 `apply_config_change` → 真实生成配置 + 起 sing-box，别在单测里碰
- handler 测试套路：`test_support::test_app(Config{..})` + `oneshot(json_request(...))`
- 新增配置字段会波及所有测试里的 `Config{..}` 字面量，sed 批量删/加即可

## 后端架构速记

- 配置变更链路（增删节点/订阅/规则都走这条）:`config_update` 锁 → 克隆配置 → 改 → `apply_config_change`(原子写 config.yaml → 生成 sing-box 配置 → `sing-box check` 校验 → 热重启，失败回滚）
- 面板不直接碰 sing-box，控制面是 Clash API(`127.0.0.1:6262`)：切节点、测延迟、连接统计都走它
- 自定义规则(`custom_rules`）插入在 sniff/hijack-dns 之后、内置分流规则**之前**,用户规则优先;全局模式下内置分流被裁掉,自定义规则依然生效
- `route_mode`（分流/全局）是会话级状态，不进配置文件

## 脚本

- `install.sh` / `remove.sh` 提交前跑 `shellcheck`
- `remove.sh` 的确认提示刻意从 `/dev/tty` 读：`curl | sudo bash` 管道场景也能提示；**别再给那个 read 加重定向**，`read -p` 的提示语走 stderr，曾被 `2>/dev/null` 吞掉导致"假卡住"
