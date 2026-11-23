# Harbor 客户端内核

[source.json](../scripts/sing-box/source.json) 固定上游 `sing-box` 仓库、完整 SHA、Go 版本、内核版本号、构建标签与能力清单。构建完全锁定源码与工具链，Harbor 发版不会自动升级内核。

## 能力与裁剪

[client.patch](../scripts/sing-box/client.patch) 裁剪组件注册，`harbor-client-v4` 仅保留客户端所需能力：

| 类别 | 保留能力 |
| --- | --- |
| 入站 | `tun`（默认 GoTUN，省略已弃用 `stack` 字段；保留 `system` 栈供手动回退）、`mixed`（局域网独立代理端口） |
| 节点出站（8 种） | SOCKS5、Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC，及配套传输、TLS/uTLS、Reality、复用与 Shadowsocks 插件 |
| 内部出站 | `direct`、`selector`、`urltest` |
| DNS | `udp`、`https`、`local`（隐式兜底） |
| 控制与缓存 | Clash API、`cache_file` |

不注册代理服务端入站、endpoint、额外服务、证书签发或其他 DNS transport。手动 JSON 仅接受上述 **8 种**节点协议：生成阶段跳过不支持的类型并告警（保留原始 JSON），内核 `check` 拒绝已移除类型。

- TUN 名为 `sing-tun`，启用 `auto_route`、`strict_route`，Linux 额外启用 `auto_redirect: true`，`multi_queue` 默认关闭。
- 内置 `direct` 配置 30 秒 TCP keepalive / 15 秒间隔，加速探测闲置连接断开。
- `harbor-kernel` 仅保留 `run` / `check` / `version` 与 namespace 辅助入口；运行时解压为 `sing-box`（Windows 为 `sing-box.exe`）。

## 补丁与回归

- **激活回执（[harbor_activation.go](../scripts/sing-box/harbor_activation.go)）**：父进程在启动或 Unix SIGHUP 前写入唯一请求 ID 与配置 SHA-256；内核在读配置前捕获请求，校验实际读取字节，并仅在实例 `Start()` 成功后原子发布带 PID 的回执。控制面须同时校验回执、进程存活与 TUN/Clash 健康。回归见 [harbor_activation_test.go](../scripts/sing-box/tests/harbor_activation_test.go) 与 [harbor_activation_unix_test.go](../scripts/sing-box/tests/harbor_activation_unix_test.go)。
- **Context 隔离回归（[harbor_context_test.go](../scripts/sing-box/tests/harbor_context_test.go)）**：验证上游 `create` / `check` 均通过 `service.ExtendContext(globalCtx)` 隔离可变注册表，避免 SIGHUP 重载串用 manager。
- **Shadowsocks 半关闭（[shadowsocks2-halfclose.patch](../scripts/sing-box/shadowsocks2-halfclose.patch)）**：为经典 AEAD 与 AEAD-2022 客户端补齐 `CloseWrite`（必要时先发加密目标地址再关底层写端，保留响应读端；不支持半关闭的底层传输回退完整关闭）。回归见 [harbor_shadowsocks_test.go](../scripts/sing-box/tests/harbor_shadowsocks_test.go)。构建脚本在临时目录用相对 `replace` 对固定的 `sing-shadowsocks2` 版本打补丁（[kernel.sh](../scripts/lib/kernel.sh) 严格校验版本，不污染共享 Go 缓存）。

## 构建与嵌入

在仓库根目录执行（需 Git、Go、Bun、curl）：

```bash
./dev.sh kernel all                   # 两架构 Linux 内核与规则
./dev.sh kernel windows-amd64         # Windows amd64 内核与规则
./dev.sh kernel amd64 --kernel-only   # 仅重建指定内核（跳过规则更新）
```

流程为：**上游回归 → host 规则编译器 → 打补丁与客户端回归（各重复 20 次）→ 目标编译与 Zstd level 19 压缩**，全部通过才写入 `embedded/`。默认同时将上游规则分支解析为完整 SHA 并写入 `rules.meta.json`。

| 产物（`embedded/`） | 用途 |
| --- | --- |
| `sing-box-<target>` / `sing-box-windows-amd64.exe` | 原始内核（供本地校验） |
| 同名 `.zst` | 嵌入 Rust 二进制的 Zstd level 19 压缩数据 |
| 同名 `.meta.json` | 源码、工具链、能力、补丁哈希、压缩前后体积与 SHA-256 |
| `sing-box-host` | 未裁剪宿主规则编译器（不嵌入成品） |

资源更新后须[重编 Harbor](development.md#本地构建与生产部署)。运行启动时以纯 Rust 流式解压到临时文件并校验完整帧、体积与 SHA-256 后原子替换（8 MiB 解码窗口）；失败保留旧内核。

## 升级与验收

1. 审查上游协议、TUN、DNS、Clash API 与依赖变更，更新 `source.json` 并递增内核版本；工具链变更须单独验证。
2. 审查补丁冲突与依赖版本（若上游已修复则移除功能补丁、保留回归测试）。
3. 运行 `./dev.sh kernel all`、`./dev.sh check`、`./dev.sh audit`，核对产物清单与哈希。
4. 运行 `./dev.sh netns` 在隔离命名空间验证启停、热重载、直连优先、来源 IP 绑定、TCP/UDP、每节点 DoH 及 DNS 缓存隔离；公网线路与 OpenWrt 真机仍需实机验证。
5. 保留对应上游源码副本（上游 testing 曾重写历史，不可回退到浮动 HEAD）；正式发布时的完整溯源清单 `release-manifest.json` 由 GitHub Actions 作为 Workflow Artifact 保留 90 天。

## 来源与许可

上游 `sing-box` 使用 **GPL-3.0-or-later**，其 README 对派生作品另有命名约定。Harbor 定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁与构建脚本共同确定。
