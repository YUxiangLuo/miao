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

### 升级至 alpha.10（miao.9）

2026-10-08，将固定源码更新为上游 `testing` 的 [`fe92ab3e78a9bb7d448c155ef6906218e2ca5453`](https://github.com/SagerNet/sing-box/commit/fe92ab3e78a9bb7d448c155ef6906218e2ca5453)，内核版本为 `1.15.0-alpha.10+miao.9.fe92ab3e`，继续使用 Go 1.27.1。相对 alpha.7 固定点，上游新增 `dns_server_address` / `dns_search_domain` 规则项（alpha.8）、升级 NaiveProxy（alpha.9）并重做 forward NAT 的 UDP 映射与分片，同时调整证书 SHA-256 固定、FakeIP 缓存一致性、DNS 嗅探、AnyTLS UDP 分帧、嵌套代理组中断和 HTTP/2 extended CONNECT；`sing`、`sing-tun`、`sing-quic`、`sing-mux`、`sing-anytls`、`sing-vmess` 等依赖升级。

`include/` 注册表未变，`client.patch` 无需调整，继续只注册既有 `miao-client-v2` 能力，不暴露 MASQUE endpoint 或 HTTP 代理。Shadowsocks 客户端依赖升级到 `sing-shadowsocks2 v0.2.2-0.20260929152114-a69d1086332b`；上游仍未提供 `CloseWrite`，`shadowsocks2-halfclose.patch` 原样适用（构建脚本中的依赖版本校验与 `replace` 目标已同步更新）。`sing-shadowsocks` 服务端依赖同步升级到 `v0.2.9-0.20260929204512-65740e0f0e3e`，与客户端依赖不可混淆。

本地验证：Linux amd64、Linux arm64、Windows amd64 三目标构建均通过；每个目标的未裁剪上游及客户端 Go 回归各重复 20 次。三个资源清单的源码版本、SHA-256 与压缩前后大小已核对，Linux 原生 `version` 显示新 SHA。Rust 495 项测试（另 1 项忽略）、Clippy、fmt 及 Bun 30 项脚本测试通过。

| 目标 | 原始内核 | 压缩内核 |
| --- | ---: | ---: |
| Linux amd64 | 24,899,708 B | 7,796,626 B |
| Linux arm64 | 23,068,796 B | 6,935,650 B |
| Windows amd64 | 24,455,680 B | 7,668,933 B |

仍需在隔离网络与 Windows/OpenWrt 真机验证 TUN、DNS、Clash API、连续重载及实际节点传输；本次上游对路由、网络栈和协议输入校验的改动尚未做网络验收。未部署生产。

### 升级至 alpha.11（miao.10）

2026-10-10，将固定源码更新为上游 `testing` 的 [`6afeff4c0f7123b5782f888812e96b8c82c7b699`](https://github.com/SagerNet/sing-box/commit/6afeff4c0f7123b5782f888812e96b8c82c7b699)，内核版本为 `1.15.0-alpha.11+miao.10.6afeff4c`，继续使用 Go 1.27.1。相对 alpha.10 改动很小（13 个文件）：`Scope.Close` 改为收集并过滤 closed/canceled 清理错误、libbox monitor 与 command server 调整、powerreport 精简，以及 macOS `systemconfig` 重写；`sing-tun` 升级到 `v0.9.7-0.20261009022811-5c2edb183cc9`，Shadowsocks 客户端依赖仍为 `sing-shadowsocks2 v0.2.2-...`。

`include/` 注册表与上一固定点一致，`client.patch` 与 `shadowsocks2-halfclose.patch` 均无需调整，`build-embedded.sh` 的依赖版本校验不变。

本地验证（Windows 版本已取消，仅 amd64/arm64）：两个目标的未裁剪上游及客户端 Go 回归各重复 20 次；资源清单的源码版本与 SHA-256 已核对，Linux 原生 `version` 显示新 SHA。`./scripts/ci.sh all`（前端 + 两目标内核 + Rust 495 项测试 + Clippy/fmt/cargo-audit）通过。

| 目标 | 原始内核 | 压缩内核 |
| --- | ---: | ---: |
| Linux amd64 | 24,899,708 B | 7,796,036 B |
| Linux arm64 | 23,068,796 B | 6,934,680 B |

`embedded/sing-box-windows-amd64.exe` 保留在上一个固定点（alpha.10）；本机不再编译 Windows 版本，需要时由参考 workflow 重建。仍需在隔离网络与真机验证 TUN、DNS、Clash API 与连续重载。未部署生产。

### 升级至 alpha.12（miao.11）

2026-10-11，将固定源码更新为上游 `testing` 的 [`e4b7e692f4545890bfc9d43960328e1449fa803b`](https://github.com/SagerNet/sing-box/commit/e4b7e692f4545890bfc9d43960328e1449fa803b)，内核版本为 `1.15.0-alpha.12+miao.11.e4b7e692`。相对 alpha.11 源码树没有变化：差异仅在 `docs/changelog.md`（新增 alpha.12 条目）和上游自己的 `.github/workflows/build.yml`，`go.mod`、`include/` 与两个补丁均未变，属版本号递增。

本地验证（Windows 已取消，仅 amd64/arm64）：两目标 Go 回归 ×20、`./scripts/ci.sh all` 通过；原始内核字节数与 alpha.11 完全相同，仅版本字符串与压缩产物变化。

| 目标 | 原始内核 | 压缩内核 |
| --- | ---: | ---: |
| Linux amd64 | 24,899,708 B | 7,796,068 B |
| Linux arm64 | 23,068,796 B | 6,935,262 B |

未部署生产。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，README 另有派生作品命名限制。定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
