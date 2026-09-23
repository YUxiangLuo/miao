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

### DNS 完成事件（miao.9）

[`dns-instrumentation.patch`](../scripts/sing-box/dns-instrumentation.patch) 在 `Router.Exchange`、`Router.ExchangeAsync` 和 `Router.Lookup` 逻辑请求边界记录一次 INFO 完成事件，不在底层 client 的竞速或推测请求上重复计数。事件以字面量 `MIAO_DNS ` 开头，后接紧凑 JSON；耗时从 Router API 入口以单调时钟计算，包含规则、缓存和去重等待。同步返回、异步早期响应与预定义响应均覆盖，异步先记录再按原顺序调用完成回调。

v1 字段为 `v`、`domain`、`query_type`、`duration_ms`、`outcome`、`rcode`、`error`、`transport`。`duration_ms` 保留亚毫秒精度；`transport` 为实际选定的 DNS transport tag，不是协议类型。Exchange 使用实际 DNS question type（未知类型使用数值），Lookup 固定为 `LOOKUP`；空或畸形请求使用空域名与 `UNKNOWN`。非零响应 RCODE 为 `dns_error`，Go error 为 `error`，但 typed `RcodeError` 仍为 `dns_error`；无法可靠确定最终 transport 时为 `null`。字段使用 `encoding/json` 编码，不从 context 前缀推算延迟，也不虚构 cache hit/miss。

示例：

```text
MIAO_DNS {"v":1,"domain":"example.com","query_type":"A","duration_ms":12.35,"outcome":"success","rcode":0,"error":null,"transport":"cfdns"}
```

目标命令 `miao-kernel` 只包含 run/check/version 与 namespace 辅助入口，直接复用上游实现；运行时文件仍叫 `sing-box` / `sing-box.exe`。CLI context 隔离修复已进入上游，Miao 仅保留[回归测试与问题说明](../scripts/patches/README.md)。

## 构建与嵌入

```bash
./scripts/build-embedded.sh --kernel-only
MIAO_TARGET=arm64 ./scripts/build-embedded.sh --kernel-only
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh --kernel-only
```

需要 Git、Go、Bun、curl。去掉 `--kernel-only` 会同时更新分流规则；`SING_GEOIP_REF` / `DIRECT_RULES_REF` 可指定分支、tag 或 SHA，Release CI 在一次发布内固定规则快照。`SING_BOX_REF` 只允许等于清单中的 SHA。

构建先验证未修改的上游并编译 host 规则编译器，再应用补丁、组装客户端命令、运行客户端回归，最后编译和压缩。客户端还应用 [`shadowsocks2-halfclose.patch`](../scripts/sing-box/shadowsocks2-halfclose.patch)：只复制固定的 `sing-shadowsocks2 v0.2.1` 到临时源码目录并用相对路径 `replace`，不修改共享 Go 缓存；依赖版本漂移时停止构建，要求重新审查补丁。全部成功后才写入 `embedded/`：

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

2026-09-20，固定上游 `testing` 的最新提交 [`fcc7e76d65257a2a43911ed0e9657a73ce004145`](https://github.com/SagerNet/sing-box/commit/fcc7e76d65257a2a43911ed0e9657a73ce004145) / Go 1.27.1 / `1.15.0-alpha.6+miao.6.fcc7e76d`，单队列（`multi_queue` 默认关闭）。上游重写了 `testing` 历史；与上一固定树 `eb57d4f5` 相比实际仅 7 个文件、33 行新增和 22 行删除，重点核对：

- TCP 流复制会穿透计数包装寻找 `CloseWrite`，gRPC 客户端单独向上游发送 `CloseSend`，修复 half-close 不能正确传播的问题。
- Reality 客户端/服务端不再声明 writer 可替换，badTLS read-wait 包装补全 writer 可替换声明；`sagernet/sing` 跟随升级到 `814d533a21ef` 以提供对应解包行为。
- 协议、TUN、DNS、Clash API 注册和 Go 工具链没有变化；原客户端补丁可直接应用，能力快照仍严格保持 `miao-client-v2` 的七种节点协议。

| 目标 | 原始内核 | 压缩内核 |
| --- | ---: | ---: |
| Linux amd64 | 24,858,748 B | 7,786,436 B |
| Linux arm64 | 23,003,260 B | 6,922,343 B |
| Windows amd64 | 24,435,200 B | 7,666,978 B |

三目标构建、每个目标的未裁剪上游隔离与能力回归 ×20、裁剪后客户端配置/出站拒绝回归 ×20、Linux 原生 `version` 冒烟、embedded 解压校验、Rust 484 项测试（1 项忽略）、Clippy、fmt、Windows core 交叉检查、Bun 16 项脚本测试及前端构建通过。已核对三个产物的源码版本、定制文件哈希和压缩前后 SHA-256，并验证 Rust 构建嵌入了新内核。

上一固定点 `eb57d4f5` 的 Linux 隔离网络验收覆盖两个独立网络命名空间、本地 DNS/TCP/UDP/Shadowsocks、自动重定向与纯 GoTUN、Clash 连接/计数、连续重载和退出清理。本次连接 half-close 变更尚未重复该网络验收；发布前仍需重跑，并在 Windows/OpenWrt 真机及实际使用的节点传输上验证。

2026-09-22 补验当前固定内核：两个独立 Linux 网络命名空间中，分别启用和关闭 `auto_redirect`，本地 DNS 劫持、direct/Shadowsocks TCP 256 KiB、UDP 64/8192/16000 B、Clash 连接链和流量计数、连续 5 次重载及退出清理均通过。三目标内嵌资源的清单及压缩前后哈希也已核对；未更换内核。

miao.6 的额外 half-close 验收**未全部通过**：本地服务在读到 EOF 后才回复，客户端 `shutdown(SHUT_WR)` 后 direct 能收到回复，但 Shadowsocks AEAD (`aes-128-gcm`) 链路收到零字节 EOF，两种 TUN 模式均可复现。当时依赖的 AEAD 包装没有提供 `CloseWrite`，不能将通用复制层的修复等同于该协议链路已支持半关闭；尚未做旧版本对照，不判定为本次升级回归。Windows/OpenWrt 真机及其他协议、传输仍待验收。

### AEAD 半关闭修复（miao.7）

保持同一上游 SHA 和 Go 工具链，客户端版本递增至 `1.15.0-alpha.6+miao.7.fcc7e76d`。实际出站依赖是 `sing-shadowsocks2 v0.2.1`；旧的 `sing-shadowsocks v0.2.8` 用于上游服务端，不能混淆两者。

补丁仅为经典 AEAD 和 AEAD-2022 客户端连接添加 `CloseWrite`：尚未发送请求时先写入加密目的地址，然后传播底层写端关闭，保留响应方向。只通过已有可替换/计数包装规则寻找半关闭能力；不将加密层标记为可绕过。不支持半关闭的底层传输保持原有完整关闭行为，不谎报成功而让对端等待 EOF。

[`miao_shadowsocks_test.go`](../scripts/sing-box/tests/miao_shadowsocks_test.go) 使用真实 loopback TCP 和独立服务端实现，覆盖空请求、多加密帧、EOF 后响应、半关闭后写入失败、不透明传输包装退回完整关闭，以及请求/FIN 错误传播。未打补丁时缺少 `CloseWrite` 的测试失败；补丁后的客户端构建将这些回归与原有能力、配置测试一起重复 20 次。不扩展为所有插件或多路复用均支持半关闭的承诺。

2026-09-22 本地验证：三目标构建及产物哈希/解压校验、每目标 Go 回归 ×20、半关闭竞态检测 ×20、Rust 490 项测试（另 1 项忽略）、Clippy、Windows core 交叉检查、Bun 23 项脚本测试和 ShellCheck 均通过。两个隔离 Linux 网络命名空间中，开启/关闭 `auto_redirect` 分别验证 direct 与 Shadowsocks：空请求和 131071 字节请求半关闭后均收到完整响应；DNS 劫持、TCP 256 KiB、UDP 64/8192/16000 B、Clash 连接链/计数、连续 5 次重载和退出清理均通过。未部署生产；Windows/OpenWrt 真机仍未验收。

### 升级至 alpha.7（miao.8）

2026-09-24，将固定源码更新为上游 `testing` 的 [`474c63c3a8828602ac6248684edd824c72e49761`](https://github.com/SagerNet/sing-box/commit/474c63c3a8828602ac6248684edd824c72e49761)，内核版本为 `1.15.0-alpha.7+miao.8.474c63c3`，继续使用 Go 1.27.1。上游新增 MASQUE endpoint、重写 HTTP 代理，并调整 DNS 查询去重、嵌套代理组连接中断、FakeIP 缓存和 TLS 证书校验；`sing` 与 `sing-tun` 依赖升级，Shadowsocks 客户端依赖仍为 `sing-shadowsocks2 v0.2.1`。Miao 的补丁随上游新增注册项调整，继续只注册既有 `miao-client-v2` 能力，不暴露 MASQUE endpoint 或 HTTP 代理。

本地验证：Linux amd64、Linux arm64、Windows amd64 三目标构建均通过；每个目标的未裁剪上游及客户端 Go 回归各重复 20 次。三个资源清单、压缩前后 SHA-256 和解压内容一致，Linux 原生 `version` 显示新 SHA。Rust 495 项测试通过（另 1 项忽略），Clippy、fmt、Windows core 交叉检查、Miao release 编译及 Bun 30 项脚本测试通过。

仍需在隔离网络与 Windows/OpenWrt 真机验证 TUN、DNS、Clash API、连续重载及实际节点传输；这些行为涉及本次上游对路由、代理组和网络栈依赖的变更。未部署生产。

### DNS 分析事件（miao.9）

保持固定源码 `474c63c3a8828602ac6248684edd824c72e49761` 和 Go 1.27.1，新增上述 Router 级 DNS 完成事件。`dns-instrumentation.patch` 纳入既有 `customizationHash()`，其内容经换行归一化后参与资源清单哈希；源码来源由固定 SHA、全部补丁及定制哈希共同确定。构建过程重复运行 `TestMiaoDNS` 回归 20 次。未部署生产，Windows/OpenWrt 真机行为仍需验收。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，README 另有派生作品命名限制。定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
