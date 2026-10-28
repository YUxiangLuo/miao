# Miao 客户端内核

[source.json](../scripts/sing-box/source.json) 统一记录上游仓库、完整 SHA、Go/内核版本、构建标签和能力清单。构建固定源码与工具链，Miao 发版或上游 testing 变化不会自动升级内核。

## 能力与裁剪

[client.patch](../scripts/sing-box/client.patch) 调整组件注册，`miao-client-v2` 保留：

| 类别 | 能力 |
| --- | --- |
| 入站 | TUN，省略已弃用的 `stack` 字段，默认 GoTUN；保留 system 栈供手动诊断回退 |
| 节点出站 | SOCKS5、Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC，及各协议支持的传输、TLS/uTLS、Reality、复用、Shadowsocks 插件 |
| 内部出站 | direct、selector、urltest |
| DNS | UDP、HTTPS、local（隐式兜底） |
| 控制与缓存 | Clash API、cache_file |

不注册代理服务端入站、endpoint、额外服务、证书签发或其他 DNS transport。手动 JSON 也只支持七种节点协议：读取/生成时跳过不支持的类型并告警，保留原始 JSON；内核 `check` 拒绝已移除类型。

TUN 名为 `sing-tun`，启用 `auto_route`、`strict_route`，使用 `auto_redirect: true`，`multi_queue` 默认关闭。内置 direct 使用 30 秒 TCP keepalive、15 秒间隔，帮助发现闲置连接失效，不保证远端连接永久存活。

`miao-kernel` 只包含 run/check/version 与 namespace 辅助入口，复用上游实现；运行时文件仍名为 `sing-box`。

## 补丁与回归

- [miao_activation.go](../scripts/sing-box/miao_activation.go) 为父子进程增加激活回执。父进程在启动/SIGHUP 前写入唯一请求 ID 与配置 SHA-256；内核在读取配置前捕获请求，校验实际读取字节，并且仅在实例 `Start()` 成功后原子发布带 PID 的回执。控制面必须同时验证回执、进程存活及 TUN/Clash 健康，旧监听器不能提前完成新事务。[miao_activation_test.go](../scripts/sing-box/tests/miao_activation_test.go) 覆盖旧请求、哈希不匹配及真实子进程的成功/拒绝重载；命令钩子与固定上游不匹配时构建立即失败。
- CLI context 隔离已由上游实现；[miao_context_test.go](../scripts/sing-box/tests/miao_context_test.go) 保留 create/check 两处注册表隔离回归。两者均须通过 `service.ExtendContext(globalCtx)` 复制可变注册表，同时保留取消语义；仅 `context.WithCancel` 会共享 manager，可能在 SIGHUP 重载时访问未启动的 AnyTLS client。测试使用 direct 和未选中的 AnyTLS，不使用 TUN/外网，也不并行修改 CLI 全局变量。
- [shadowsocks2-halfclose.patch](../scripts/sing-box/shadowsocks2-halfclose.patch) 为经典 AEAD、AEAD-2022 客户端提供 `CloseWrite`：必要时先发送加密目的地址，再关闭底层写端并保留响应方向。不绕过加密层；不支持半关闭的底层传输退回完整关闭。
- [miao_shadowsocks_test.go](../scripts/sing-box/tests/miao_shadowsocks_test.go) 使用 loopback TCP 和独立服务端，覆盖空请求、多帧、EOF 后响应、半关闭后写入失败、包装回退与错误传播；不承诺所有插件/复用都支持半关闭。

构建脚本将固定的 `sing-shadowsocks2` 客户端依赖复制到临时目录，用相对 `replace` 应用补丁，不修改共享 Go 缓存。精确依赖版本由 [内核构建实现](../scripts/lib/kernel.sh) 校验，漂移即停止；勿与上游服务端依赖 `sing-shadowsocks` 混淆。

## 构建与嵌入

在仓库根目录执行，需要 Git、Go、Bun、curl：

```bash
./dev.sh kernel all                # 两架构内核与规则
./dev.sh kernel amd64 --kernel-only # 仅 amd64 内核
```

默认同时更新规则，将上游分支解析为完整 SHA，并将来源和规则内容哈希写入 `rules.meta.json`。`--kernel-only` 跳过 host 规则编译器与规则更新；`SING_BOX_REF` 只能等于清单 SHA。

流程为上游回归 → host 规则编译器 → 补丁与客户端命令 → 客户端回归 → 目标编译/压缩，全部成功才写入 `embedded/`。回归与规则编译器固定使用本机 Linux 架构，目标内核按命令参数构建，不受外部 `GOARCH`/`GOOS` 影响。上游与客户端 Go 回归各重复 20 次，验证注册能力、含 TUN 配置及已移除出站的拒绝行为。

| 产物 | 用途 |
| --- | --- |
| `sing-box-<target>` | 原始内核，供验证 |
| 同名 `.zst` | Rust 嵌入的 Zstandard level 19 数据 |
| 同名 `.meta.json` | 源码、工具链、能力、定制文件哈希、压缩前后大小与 SHA-256 |
| `sing-box-host` | 未裁剪的规则编译器，不嵌入成品 |

资源变更后须[重编 Miao](development.md#构建发布与部署)。启动时以纯 Rust 解压到同目录临时文件，校验完整帧、大小和 SHA-256，再设置权限并原子替换；失败保留旧内核，不清理缓存/配置。压缩只减少分发体积，8 MiB 解码窗口与启动峰值 RSS 须在 OpenWrt 验收。

## 升级与验收

1. 审查协议、TUN、DNS、Clash API 与依赖变化，修改 `source.json`、递增内核版本；工具链须单独验证。
2. 审查补丁冲突与依赖版本；上游已有等效修复时删除功能补丁，保留行为回归。
3. 执行 `./dev.sh kernel all`、`./dev.sh check`、`./dev.sh audit`，核对两目标版本、清单与哈希。
4. 在隔离环境验证 TUN/DNS、Clash、连续重载、退出清理与实际协议传输；OpenWrt 另需真机验收。编译、`version` 和解压测试不能替代网络验收。
5. 本地保留对应源码，以及 `dist/vX.Y.Z/release-manifest.json` 中的内核、规则来源与哈希。该清单不上传 Release；仓库 tag 的源码包包含内核固定来源、补丁、构建脚本与许可。

testing 曾重写历史：固定 SHA 不保证对象永久可下载，须保留可恢复的源码副本；对象失效时先恢复镜像，再改 repository，不回退到 testing HEAD。Git bundle 不包含 Go 模块依赖。

## 验证范围

`./dev.sh netns` 验证 Miao 与内核的控制流程、TUN 创建及清理，并用两个客户端和两个真实 VLESS 服务端验证按来源 IP 固定出口、TCP/UDP、每节点 DoH，以及删除节点后的 DNS 缓存隔离。默认分支保留自定义规则优先；工作室分支使用大陆直连优先，各自断言对应行为。服务端由 `embedded/sing-box-host` 提供，所有目标及证书均为隔离网络内的本地测试材料。它不验证公网线路、其他协议或 OpenWrt 真机；这些仍需要专用环境。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，README 另有派生作品命名限制。定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
