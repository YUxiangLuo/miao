# Miao 客户端内核

[`source.json`](../scripts/sing-box/source.json) 是上游仓库、完整 SHA、Go 版本、内核版本、构建标签和能力清单的唯一入口。构建始终使用指定工具链；普通 Miao 发版和 testing 分支变化不会自动升级内核。

## 能力与裁剪

[`client.patch`](../scripts/sing-box/client.patch) 调整组件注册，`miao-client-v2` 保留：

| 类别 | 保留能力 |
| --- | --- |
| 入站 | TUN，省略已弃用的 `stack` 字段，使用上游默认 GoTUN；保留 system 栈供诊断回退，不自动切换 |
| 节点出站 | Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC，以及这些协议的传输、TLS/uTLS、Reality、复用和 Shadowsocks 插件 |
| 内部出站 | direct、selector、urltest |
| DNS | UDP、HTTPS、local（隐式兜底） |
| 控制与缓存 | Clash API、cache_file |

代理服务端入站、endpoint、额外服务、证书签发和其他 DNS transports 不注册。手动 JSON 同样只支持上述七种节点协议：读取或生成配置时跳过不支持的类型并告警，保留用户原始 JSON；内核 `check` 也会拒绝已移除类型。

Miao 的 TUN 使用 `sing-tun`、`auto_route: true`、`strict_route: true`，仅 Linux 写入 `auto_redirect: true`；`multi_queue` 沿用默认关闭。

内置 `direct` 出站使用 `tcp_keep_alive: "30s"`、`tcp_keep_alive_interval: "15s"`，缓解 CDN/NAT 闲置连接失效后，浏览器复用旧连接时的等待。保活用于维持路径并及时发现失效，不保证远端连接永久存活。

目标命令 `miao-kernel` 只包含 run/check/version 与 namespace 辅助入口，直接复用上游实现；运行时文件仍叫 `sing-box` / `sing-box.exe`。CLI context 隔离修复已进入上游，Miao 仅保留[回归测试与问题说明](../scripts/patches/README.md)。

## 构建与嵌入

```bash
./scripts/build-embedded.sh --kernel-only
MIAO_TARGET=arm64 ./scripts/build-embedded.sh --kernel-only
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh --kernel-only
```

需要 Git、Go、Bun、curl。去掉 `--kernel-only` 会同时更新分流规则；`SING_GEOIP_REF` / `DIRECT_RULES_REF` 可指定分支、tag 或 SHA，Release CI 在一次发布内固定规则快照。`SING_BOX_REF` 只允许等于清单中的 SHA。

构建先验证未修改的上游并编译 host 规则编译器，再应用补丁、组装客户端命令、运行客户端回归，最后编译和压缩。全部成功后才写入 `embedded/`：

| 文件 | 用途 |
| --- | --- |
| `sing-box-<target>`（Windows 加 `.exe`） | 原始目标内核，用于验证 |
| 同名 `.zst` | Rust 实际嵌入的 Zstandard level 19 数据 |
| 同名 `.meta.json` | 源码、Go、tags、能力、定制文件哈希，以及压缩前后大小和 SHA-256 |
| `sing-box-host` | 未裁剪的规则编译器，不嵌入成品 |

内核变更后须重新构建 embedded 和 Miao；只重编 Rust 不会更新已有内核资源。完整构建和部署见[开发指南](../DEV_NOTES.md#构建发布与部署)。Rust release 使用 `opt-level = "s"`、thin LTO、单 codegen unit、符号剥离，保留 panic unwind。

启动时用纯 Rust 流式解压到同目录临时文件，验证完整帧、大小和 SHA-256，设置权限后原子替换。损坏、截断或写入失败保留旧内核；缓存和配置不清理。Windows 文件被残留进程锁定时仍报错。压缩只缩小分发体积，不能据此推算运行 RSS；当前解码窗口为 8 MiB，OpenWrt 需验收启动峰值内存。

## 升级与验收

1. 审查协议、TUN、DNS、Clash API 及依赖变化，修改 `source.json`，递增内核版本。源码与 Go 可分别升级；`go.mod` 的最低版本不等于所选工具链已获验证。
2. 审查客户端补丁。冲突必须处理；上游合入等效修复后删除功能补丁，保留行为回归。
3. 执行上面的三目标构建及[开发检查](../DEV_NOTES.md#开发检查)。构建会将隔离、能力和配置回归重复 20 次：精确能力集合须为上游子集，仅暴露 TUN 入站，并验证 11 组含 TUN 的客户端配置与 9 种已移除出站（含 Tailcat）的拒绝行为。
4. 在隔离环境验收 TUN/DNS 分流、Clash 面板和连续重载；Windows/OpenWrt 还需真机验证。CI 的原生 `version`、交叉编译及 Rust 真实内核解压测试不替代网络验收。
5. 发布时保留对应源码和产物清单。Release 附带 `miao-embedded-sources.txt` 与各平台 `miao-kernel-*.json`。

testing 曾重写历史，固定 SHA 只能保证内容，不能保证对象永久可下载。审查时保留可独立恢复的源码副本；上游对象不可用时先恢复镜像，再修改 repository，不回退到 testing HEAD。本机已验证的 Git bundle 只包含上游源码与历史，不包含 Go 模块依赖。

## 当前基线验证记录

2026-09-20，固定上游 `testing` 的最新提交 [`eb57d4f5293fbec9417be18237a81d30824b8285`](https://github.com/SagerNet/sing-box/commit/eb57d4f5293fbec9417be18237a81d30824b8285) / Go 1.27.1 / `1.15.0-alpha.6+miao.5.eb57d4f5`，单队列（`multi_queue` 默认关闭）。相对上一固定点 `f6ce1d5b` 共 51 个提交、159 个文件变化，重点核对：

- DNS 查询去重、DNS 嗅探误判修复、隐式 DNS/出站初始化、连接取消与空闲连接管理，以及 Windows 进程查询变化。
- sing-tun 升至 `v0.9.6-0.20260919141142-a39eab51450b`，涉及 GoTUN TCP/UDP、内存设备、Windows I/O、Linux/Android 自动重定向；sing 和 sing-mux 同步跟随上游依赖。
- 上游弃用 `stack`；Miao 新生成配置省略该字段。已确认 sing-tun 的空值和 `go` 均选择 `NewGo`，已有 `stack: "go"` 缓存在本版本仍可读取。
- 上游新增 Tailcat 入站/出站注册，裁剪补丁已同步移除，保持 `miao-client-v2` 的七种节点协议。CLI context 隔离修复仍由上游提供。

| 目标 | 原始内核 | 压缩内核 |
| --- | ---: | ---: |
| Linux amd64 | 24,858,748 B | 7,786,017 B |
| Linux arm64 | 23,003,260 B | 6,921,912 B |
| Windows amd64 | 24,436,224 B | 7,668,239 B |

Arch 原生完整 Miao 为 **16,465,904 B / 15.70 MiB**（1 MiB = 1,048,576 B）。三目标构建、未裁剪上游隔离与能力回归 ×20、裁剪后客户端配置/出站拒绝回归 ×20、Linux 原生 `version` 冒烟、embedded 解压校验、Rust 476 项测试（1 项忽略）、Clippy、fmt、Windows core 交叉检查、Bun 脚本测试及前端构建通过。已核对三个产物的源码版本、定制文件哈希和压缩前后 SHA-256，并验证完整 Miao 嵌入了新内核。

Linux 隔离网络验收通过：两个独立网络命名空间连接本地 DNS、TCP/UDP echo 和 Shadowsocks 服务端，分别验证 `auto_redirect: true` 和关闭自动重定向的纯 GoTUN 路径；两种模式均覆盖 DNS 劫持、直连/代理分流、MTU 9000 下 TCP 256 KiB 与 UDP 64/8192/16000 B 往返、Clash 连接链及流量计数、连续 5 次 SIGHUP 后重复验收，以及退出时 TUN/nftables/策略路由清理。测试没有连接外部代理或改动本机生产服务；Windows/OpenWrt 仍需真机验证，Linux 结果也不替代其他六种节点协议的真实网络互通测试。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，README 另有派生作品命名限制。定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
