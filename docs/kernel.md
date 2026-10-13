# Miao 客户端内核

[source.json](../scripts/sing-box/source.json) 统一记录上游仓库、完整 SHA、Go/内核版本、构建标签和能力清单。构建固定源码与工具链，Miao 发版或上游 testing 变化不会自动升级内核。

## 能力与裁剪

[client.patch](../scripts/sing-box/client.patch) 调整组件注册，`miao-client-v2` 保留：

| 类别 | 能力 |
| --- | --- |
| 入站 | TUN，省略已弃用的 `stack` 字段，默认 GoTUN；保留 system 栈供手动诊断回退 |
| 节点出站 | Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC，及其传输、TLS/uTLS、Reality、复用、Shadowsocks 插件 |
| 内部出站 | direct、selector、urltest |
| DNS | UDP、HTTPS、local（隐式兜底） |
| 控制与缓存 | Clash API、cache_file |

不注册代理服务端入站、endpoint、额外服务、证书签发或其他 DNS transport。手动 JSON 也只支持七种节点协议：读取/生成时跳过不支持的类型并告警，保留原始 JSON；内核 `check` 拒绝已移除类型。

TUN 名为 `sing-tun`，启用 `auto_route`、`strict_route`，仅 Linux 写入 `auto_redirect: true`，`multi_queue` 默认关闭。内置 direct 使用 30 秒 TCP keepalive、15 秒间隔，帮助发现闲置连接失效，不保证远端连接永久存活。

`miao-kernel` 只包含 run/check/version 与 namespace 辅助入口，复用上游实现；运行时文件仍名为 `sing-box` / `sing-box.exe`。

## 补丁与回归

- CLI context 隔离已由上游实现；保留 [create/check 注册表隔离回归](../scripts/patches/README.md)，不恢复旧功能补丁。
- [shadowsocks2-halfclose.patch](../scripts/sing-box/shadowsocks2-halfclose.patch) 为经典 AEAD、AEAD-2022 客户端提供 `CloseWrite`：必要时先发送加密目的地址，再关闭底层写端并保留响应方向。不绕过加密层；不支持半关闭的底层传输退回完整关闭。
- [miao_shadowsocks_test.go](../scripts/sing-box/tests/miao_shadowsocks_test.go) 使用 loopback TCP 和独立服务端，覆盖空请求、多帧、EOF 后响应、半关闭后写入失败、包装回退与错误传播；不承诺所有插件/复用都支持半关闭。

构建脚本将固定的 `sing-shadowsocks2` 客户端依赖复制到临时目录，用相对 `replace` 应用补丁，不修改共享 Go 缓存。精确依赖版本由 [build-embedded.sh](../scripts/build-embedded.sh) 校验，漂移即停止；勿与上游服务端依赖 `sing-shadowsocks` 混淆。

## 构建与嵌入

在仓库根目录执行，需要 Git、Go、Bun、curl：

```bash
./scripts/build-embedded.sh --kernel-only
MIAO_TARGET=arm64 ./scripts/build-embedded.sh --kernel-only
# 仅维护 Windows 时需要，当前发布流程不构建此目标
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh --kernel-only
```

去掉 `--kernel-only` 同时更新规则；`SING_GEOIP_REF` / `DIRECT_RULES_REF` 接受分支、tag、SHA，发布脚本会固定本次规则快照。`SING_BOX_REF` 只能等于清单 SHA。

流程为上游回归 → host 规则编译器 → 补丁与客户端命令 → 客户端回归 → 目标编译/压缩，全部成功才写入 `embedded/`。上游与客户端 Go 回归各重复 20 次，验证注册能力、含 TUN 配置及已移除出站的拒绝行为。

| 产物 | 用途 |
| --- | --- |
| `sing-box-<target>`（Windows 加 `.exe`） | 原始内核，供验证 |
| 同名 `.zst` | Rust 嵌入的 Zstandard level 19 数据 |
| 同名 `.meta.json` | 源码、工具链、能力、定制文件哈希、压缩前后大小与 SHA-256 |
| `sing-box-host` | 未裁剪的规则编译器，不嵌入成品 |

资源变更后须[重编 Miao](../DEV_NOTES.md#构建发布与部署)。启动时以纯 Rust 解压到同目录临时文件，校验完整帧、大小和 SHA-256，再设置权限并原子替换；失败保留旧内核，不清理缓存/配置。Windows 文件被锁定时仍报错。压缩只减少分发体积，8 MiB 解码窗口与启动峰值 RSS 须在 OpenWrt 验收。

## 升级与验收

1. 审查协议、TUN、DNS、Clash API 与依赖变化，修改 `source.json`、递增内核版本；工具链须单独验证。
2. 审查补丁冲突与依赖版本；上游已有等效修复时删除功能补丁，保留行为回归。
3. 执行 `./scripts/ci.sh all`，核对各目标版本、清单与哈希。维护 Windows 时另做该目标构建、core 交叉检查和桌面检查。
4. 在隔离环境验证 TUN/DNS、Clash、连续重载、退出清理与实际协议传输；OpenWrt/Windows 另需真机验收。编译、`version` 和解压测试不能替代网络验收。
5. 发布保留对应源码与清单：`miao-embedded-sources.txt`、各平台 `miao-kernel-*.json`。

testing 曾重写历史：固定 SHA 不保证对象永久可下载，须保留可恢复的源码副本；对象失效时先恢复镜像，再改 repository，不回退到 testing HEAD。Git bundle 不包含 Go 模块依赖。

## 当前基线验证记录

以下是已有验收记录，历史细节可用 `git log -p -- docs/kernel.md` 查看：

| 日期 / 基线 | 已验证范围与限制 |
| --- | --- |
| 2026-09-22，miao.7 / `fcc7e76d` | 修复 miao.6 的 Shadowsocks AEAD 半关闭失败；三目标构建、Go 回归与竞态检查通过。两个 Linux 网络命名空间、两种 TUN 模式下验证 DNS、direct/SS TCP/UDP、空请求与大请求半关闭、Clash 计数、连续 5 次重载、退出清理 |
| 2026-10-08，miao.9 / `fe92ab3e` | 三目标构建与 Go/Rust 回归通过；Windows 最近一次内核构建基线，不能代表后续固定点已通过 Windows 验证 |
| 2026-10-11，miao.11 / `e4b7e692` | 当前固定点；Linux amd64/arm64 Go 回归 ×20、`./scripts/ci.sh all` 通过。相对 miao.10 只更新版本、上游文档与 workflow，源码实现和补丁未变 |

最近完整 Linux 隔离网络验收仍是 miao.7；后续内核的 TUN、DNS、Clash、连续重载与实际传输须重新验收，Windows/OpenWrt 真机也仍待验收。上述记录不表示已部署生产；体积与哈希以对应产物清单为准。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，README 另有派生作品命名限制。定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
