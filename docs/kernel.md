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

### Clash 快照序列化（miao.10）

保持上游 SHA、Go 版本和采集频率不变。[`clash-snapshot.patch`](../scripts/sing-box/clash-snapshot.patch) 将每条连接的动态 map 与自定义 `MarshalJSON` 改为明确的 JSON 结构体，直接参与整个快照的编码，避免逐条生成 JSON 缓冲区后再次扫描。HTTP 和 WebSocket 共用这条路径；不切换为可能丢失事件的增量订阅，不减少日志、进程归属或流量计数。

JSON 对象键的顺序可以变化，字段、值和类型保持兼容，包括字符串端口、无效 IP 的空字符串、空列表与 `null` 的区别、纳秒时间、int64 计数及进程名称回退顺序。计数仍为实时采样，并非所有连接在同一时刻的事务快照。构建将 [`miao_connections_test.go`](../scripts/sing-box/tests/miao_connections_test.go) 复制到 `experimental/clashapi`，重复验证真实 HTTP/WebSocket handler、DNS 连接过滤和关闭后的总计 20 次。

Linux amd64 / Ryzen 5 5600 / Go 1.27.1，单 benchmark goroutine、默认 GOMAXPROCS=12；每项 `500ms × 6`，以下为中位数。两组使用相同的合成连接、进程信息与固定计数，衡量**快照转换与 JSON 编码**，不含连接枚举、`Manager.Total()`、HTTP 传输或实际代理流量；分配字节不是 RSS。

| 连接数 | 修改前耗时 | 修改后耗时 | 修改前分配/次 | 修改后分配/次 |
| --- | ---: | ---: | ---: | ---: |
| 1 | 8.91 µs | 2.80 µs | 5,399 B | 1,730 B |
| 100 | 0.800 ms | 0.154 ms | 483,540 B | 107,834 B |
| 1,000 | 8.826 ms | 1.667 ms | 5,082,004 B | 1,277,422 B |
| 10,000 | 76.654 ms | 14.654 ms | 58,926,808 B | 15,695,395 B |

1,000 条连接时，分配次数从约 72,039 降至 16,021。不能将这些数字解释为整机 CPU 或网络吞吐提高相同比例；Windows/OpenWrt 的收益仍需真机测量。

复测：在独立的固定上游源码副本中应用客户端、DNS 和快照补丁，复制上述测试文件，然后运行（不能向正在提供网络的 Miao 实例发压测请求）：

```bash
GOTOOLCHAIN=go1.27.1 CGO_ENABLED=0 go test -mod=readonly \
  -tags with_quic,with_clash_api,with_utls ./experimental/clashapi \
  -run '^TestMiaoConnection' -count=20
GOTOOLCHAIN=go1.27.1 CGO_ENABLED=0 go test -mod=readonly \
  -tags with_quic,with_clash_api,with_utls ./experimental/clashapi \
  -run '^$' -bench '^BenchmarkMiaoConnections$' -benchmem -benchtime=500ms -count=6
```

修改前对照使用另一个相同源码副本，仅不应用 `clash-snapshot.patch`；复制同一测试文件，并在该包新增仅供基准兼容的 `miao_connections_baseline_test.go`，内容如下。不要将适配器带入优化后的源码或发行包。

```go
package clashapi

import "github.com/sagernet/sing-box/common/trafficcontrol"

func newConnectionObject(c *trafficcontrol.TrackerMetadata) connectionObject {
    return connectionObject(*c)
}
```

本地验证：Linux amd64、Linux arm64、Windows amd64 三目标完整内核构建通过，每次构建均执行 Go 回归 ×20；Clash 回归额外通过 `-race -count=20`。Linux 原生 `version`、Rust 嵌入资源测试 3 项、Bun 脚本测试 30 项及 ShellCheck 通过。未部署或重启正在提供网络的实例；跨平台编译不替代 Windows/OpenWrt 真机验证。

### Linux 进程扫描合并（miao.11）

[`process-scan.patch`](../scripts/sing-box/process-scan.patch) 按 UID 合并同时进行的 `/proc` 扫描，只共享包含目标 inode 的结果。共享结果未命中或报错时，等待者执行自己的查询，再沿用原有的全用户扫描回退；否则，扫描开始后创建的 socket 可能被旧快照漏掉。不延长一秒缓存、不增加负缓存、不禁用进程识别，也不修改 netlink 与路由层的连接元组缓存。不同 UID 的扫描不互相串行等待。

[`miao_process_linux_test.go`](../scripts/sing-box/tests/miao_process_linux_test.go) 用 `testing/synctest` 精确控制共享扫描的发布时机，结合真实 loopback UDP socket 与 `/proc/self/exe` 验证进程归属。覆盖 32 个等待者共享成功结果、扫描期间创建 socket、全用户回退、共享错误后的重查、缓存过期和清空。故意删除重查时，三个新 socket 测试均失败。Linux 构建主机重复运行这些回归 20 次；Windows 主机不执行 Linux 测试。

本机固定负载基准：Linux amd64 / Ryzen 5 5600 / Go 1.27.1 / GOMAXPROCS=12，独立 PID、mount 和网络命名空间，64 个 `sleep` 进程各保留 64 个 `/dev/null` FD。每项 `500ms × 5`，取中位数。测试在同一二进制内对比固定上游实现与补丁后的实际查询函数；32 路命中使用不同的真实 socket，每次检查返回路径，不只检查无报错。未命中使用不存在的 inode 0，强制走 UID 和全用户扫描。

| 负载 | 上游耗时/次 | 优化后耗时/次 | 上游分配/次 | 优化后分配/次 |
| --- | ---: | ---: | ---: | ---: |
| 已缓存的单次查询 | 63.61 ns | 63.78 ns | 0 B | 0 B |
| 单次冷查 | 11.782 ms | 11.723 ms | 612,986 B | 612,696 B |
| 32 路同时冷查，全部命中 | 59.778 ms | 12.596 ms | 20,285,072 B | 626,511 B |
| 32 路同时冷查，全部未命中 | 110.542 ms | 118.105 ms | 40,110,097 B | 40,255,491 B |

**取舍：**并发命中时耗时约降 79%、分配约降 97%；缓存命中与单次冷查基本不变。全部未命中时仍要各自扫描，先等待共享结果使该负载约慢 7%，不能宣称所有场景都更快。两轮宿主机 `/proc` 测量也观察到并发命中的明显收益与全未命中的退化，但进程/FD 数量变化较大，不用于固定负载对照。以上不包含 netlink 查询时间，不等于整个连接建立或代理吞吐的改进。

复测时，在应用该补丁的独立固定源码副本中，将测试文件复制到 `common/process/`。普通基准命令为：

```bash
GOTOOLCHAIN=go1.27.1 CGO_ENABLED=0 go test -mod=readonly ./common/process \
  -run '^$' -bench '^BenchmarkMiaoProcessPaths$' -benchmem -benchtime=500ms -count=5
```

固定负载对照使用如下隔离方式（需要 Linux、sudo、unshare、ip；不操作宿主网络，命名空间退出会清理其子进程）：

```bash
binary=$(mktemp)
GOTOOLCHAIN=go1.27.1 CGO_ENABLED=0 go test -mod=readonly -c -o "$binary" ./common/process
sudo unshare --pid --fork --mount --mount-proc --net bash -c '
  set -e
  ip link set lo up
  for ((i=0;i<64;i++)); do
    bash -c '\''for ((j=0;j<64;j++)); do exec {fd}</dev/null; done; exec sleep 300'\'' &
  done
  until [[ $(grep -l "^sleep$" /proc/[0-9]*/comm 2>/dev/null | wc -l) == 64 ]]; do sleep 0.05; done
  exec "$1" -test.run="^$" -test.bench="^BenchmarkMiaoProcessPaths$" \
    -test.benchmem -test.benchtime=500ms -test.count=5
' _ "$binary"
rm -f "$binary"
```

本地验证：三目标完整内核构建与各次 Go 回归 ×20 通过；进程回归额外通过 `-race -count=20`，以及 `-race -count=10 -cpu=1,2,12`。资源清单、定制哈希、压缩/解压哈希、Linux 原生版本、3 项 Rust 嵌入资源测试、30 项 Bun 脚本测试及 ShellCheck 均通过。未部署生产；未在 OpenWrt/Windows 真机验收，Windows 的进程查询实现未修改。

### 同步日志分配优化（miao.12）

[`log-allocation.patch`](../scripts/sing-box/log-allocation.patch) 保留同步输出、日志级别和启动前排队行为。单个 string 参数直接作为消息，多参数及其他类型仍交给原有 `format.ToString`；输出使用标准库 `io.WriteString`，让支持 `io.StringWriter` 的目标（包括 stdout/stderr 的 `os.File`）避免整行 string → byte slice 复制，不支持时仍回退到 `Write`。DNS 完成事件先将固定前缀和 JSON 合成一个字符串，仍使用 `encoding/json`，不修改 v1 字段、转义、错误分类和完成时机。

未使用 `unsafe`、缓冲队列或异步日志，也未降级或采样日志。慢输出仍阻塞调用者；优化分配不解决管道写满或数据库积压。原有写错误处理、FATAL/PANIC 行为不变，不新增无损保证。

本机 Linux amd64 / Ryzen 5 5600 / Go 1.27.1 / GOMAXPROCS=12，独立固定源码副本中的真实 logger、关闭颜色和时间戳、模拟同步输出目标。优化前后测试二进制交替运行 6 轮，每项 200 ms，以下为中位数。DNS 使用含全部 v1 字段及引号转义的固定事件，计入 JSON 编码；对比原来的双参数调用与优化后的单字符串调用。

| 输出目标与负载 | 优化前 | 优化后 | 分配字节/次 | 分配次数/次 |
| --- | ---: | ---: | ---: | ---: |
| StringWriter：单字符串日志 | 172.4 ns | 120.7 ns | 192 → 112 B | 4 → 2 |
| StringWriter：多参数日志 | 249.8 ns | 220.9 ns | 256 → 208 B | 5 → 4 |
| StringWriter：DNS JSON 日志 | 1182.5 ns | 1004.2 ns | 1056 → 720 B | 10 → 7 |
| 仅 Write：单字符串日志 | 173.0 ns | 146.4 ns | 192 → 160 B | 4 → 3 |
| 仅 Write：多参数日志 | 244.3 ns | 260.0 ns | 256 → 256 B | 5 → 5 |
| 仅 Write：DNS JSON 日志 | 1178.0 ns | 1060.5 ns | 1056 → 880 B | 10 → 8 |

仅 Write 的多参数路径无分配收益，本次中位数约慢 6%；不能宣称所有 writer 都更快。基准不包含时间戳、context ID、observable 订阅、实际系统调用、磁盘或数据库写入，不代表 DNS 延迟或代理吞吐的对应改善。特意不用 `io.Discard` 本身作为目标，因为上游会跳过其控制台格式化路径。

复测：在独立固定源码副本中将 [`miao_log_test.go`](../scripts/sing-box/tests/miao_log_test.go) 复制到 `log/`，应用日志补丁前后分别运行：

```bash
GOTOOLCHAIN=go1.27.1 go test -mod=readonly ./log \
  -run '^TestMiaoLog' -bench '^BenchmarkMiaoLog$' -benchmem -benchtime=200ms -count=6
```

构建脚本将日志兼容性与同步背压回归重复执行 20 次；另在隔离源码副本中对日志和 DNS 回归执行 `-race -count=20`。三目标构建通过，但未部署或重启正在提供网络的实例；Windows/OpenWrt 真机及端到端吞吐仍未验证。

### GoTUN 多队列验证：保持默认关闭

固定 `sing-tun` 版本 [`041629c89d77`](https://github.com/SagerNet/sing-tun/commit/041629c89d77da4857121245dc897738ba9f3c24) 的 Linux Go 栈在 `multi_queue: true` 时按 `GOMAXPROCS` 创建队列/engine；不支持的接口可能回退到单队列。Linux `auto_redirect` 的 TCP 重定向走原生 TCP listener，不经过 GoTUN 数据路径，不能把这条路径的 TCP 吞吐变化归因于多队列。UDP 仍经过 TUN。Miao 当前不改变 `multi_queue` 默认值，也不将 Linux 专用选项写入 Windows 配置。

[`benchmark-tun.sh`](../scripts/benchmark-tun.sh) 提供可复现的隔离初筛：独立 PID/mount/network namespace，另一个 network namespace 运行本地 Shadowsocks 与 iperf3 服务端。只改变测试命名空间的路由/接口/防火墙，不连接生产 API 或数据库；PID namespace 退出清理所有测试子进程。使用临时测试凭据，不需要实际节点。结果目录必须为空，避免覆盖旧数据。

```bash
# Linux amd64，需要 sudo、iproute2、iperf3、jq、util-linux，以及已构建的
# embedded/sing-box-amd64 和完整上游 embedded/sing-box-host。
bash scripts/benchmark-tun.sh
```

本机 Ryzen 5 5600、Linux 7.2.5、miao.12、iperf3 3.21。两端 Go 进程均为 `GOMAXPROCS=4`，MTU 9000，IPv4 / Shadowsocks AES-128-GCM；客户端 trace 日志写入测试文件。每项 2 秒，单/8 流、正/反向，UDP payload 1200 B，总发送目标 200 Mbps / 2 Gbps（不是每流该速率）。共 3 轮、144 项，轮次间反转单/多队列顺序；以下接收端吞吐与丢包率各取中位数。

| 路径与负载 | 单队列 | 4 队列 | UDP 丢包率：单 → 多 |
| --- | ---: | ---: | ---: |
| 自动重定向，8 TCP 正向（非 GoTUN 数据路径） | 28.13 Gbps | 29.04 Gbps | — |
| 纯 GoTUN，8 TCP 正向 | 29.15 Gbps | 23.75 Gbps | — |
| 纯 GoTUN，8 TCP 反向 | 22.98 Gbps | 19.19 Gbps | — |
| 自动重定向，8 UDP 正向，200 Mbps | 199.97 Mbps | 199.97 Mbps | 0% → 0% |
| 自动重定向，8 UDP 正向，2 Gbps | 1.858 Gbps | 1.800 Gbps | 7.07% → 9.51% |
| 自动重定向，8 UDP 反向，2 Gbps | 1.848 Gbps | 1.816 Gbps | 7.58% → 9.16% |
| 纯 GoTUN，8 UDP 正向，2 Gbps | 1.837 Gbps | 1.902 Gbps | 8.11% → 4.88% |
| 纯 GoTUN，8 UDP 反向，2 Gbps | 1.956 Gbps | 1.990 Gbps | 2.18% → 0.47% |

自动重定向的 200 Mbps / 8 UDP 正向负载中，内核进程 CPU 中位数从 37.48% 到 47.47%（100% 为一核），没有吞吐收益。纯 GoTUN 高负载 UDP 有局部收益，但并行 TCP 退化，本轮证据不足以支持默认开启。**不是证明多队列普遍更差，也不是给所有设备推荐固定队列数。**

脚本逐次核对 `/proc/<pid>/fdinfo` 中的实际 TUN 队列数为 1/4，并验证退出后 TUN 消失；保存 iperf JSON、TUN 字节计数及进程 CPU/RSS 原始数据。144 项数据的接口计数交叉检查通过：自动重定向 TCP 未走 TUN 大流量路径，其余项均经过 TUN。ShellCheck 通过。

这只是本机短时初筛：未固定 CPU affinity，客户端、服务端和压测器共享 CPU，没有置信区间；RSS 是测试后快照，不是稳态或峰值。未覆盖 IPv6、UDP 分片、尾延迟、WAN 丢包、其他节点协议或 OpenWrt 真机，未模拟 Miao 的 DuckDB 采集压力。正式启用前仍需针对目标设备补测，不依据本表修改生产默认值。

### 嗅探：保留范围，减少单缓冲区临时对象（miao.13）

Miao 仍将无条件 `sniff` 放在 DNS 劫持和自定义域名规则之前，保留上游默认嗅探器、顺序、server-first 端口跳过及 300 ms 默认读取超时。**不按 80/443/53 等端口裁剪，也不因 DNS reverse mapping 已提供域名就跳过。**非标准端口同样可能承载 HTTP/TLS/DNS；共享 IP 的映射域名还可能被实际 SNI/Host 覆盖，跳过会改变后续分流。UDP/QUIC 嗅探及其分片状态没有改动。

[`sniff-reader.patch`](../scripts/sing-box/sniff-reader.patch) 只修改 `PeekStream`：没有前置缓存块时，每个嗅探器直接获得当前缓冲区的全新 `bytes.Reader`，省去组合读取器及中间列表；有前置缓存块时保留原路径。各嗅探器仍从头读取同一份字节，多次读取继续累积数据，原始载荷不被消费或改写。

Linux amd64 / Ryzen 5 5600 / Go 1.27.1，优化前后独立测试二进制交替运行 6 轮、每项 200 ms 的中位数如下。使用真实 HTTP、SSH 报文和标准库生成的 TLS ClientHello，调用默认六个 TCP 嗅探器；连接读取由内存替身提供，计入解析和元数据分配，不包含真实网络等待或路由日志。

| 无前置缓存块 | 优化前 | 优化后 | 分配次数/次 | 分配字节/次 |
| --- | ---: | ---: | ---: | ---: |
| HTTP | 3.311 μs | 3.142 μs | 35 → 30 | 8709 → 8621 B |
| TLS | 143.296 μs | 146.155 μs | 67 → 65 | 20868 → 20828 B |
| SSH | 5.518 μs | 5.104 μs | 90 → 76 | 约 14297 → 14068 B |

HTTP/SSH 本次约快 5%/8%；TLS 仅减少少量分配，耗时中位数反而约慢 2%，不宣称 TLS 加速。有前置缓存块的对照分配次数不变，耗时基本不变。这是局部小优化，不等于连接延迟或代理吞吐的同幅提升，也没有消除等待首包的延迟。

[`miao_sniff_test.go`](../scripts/sing-box/tests/miao_sniff_test.go) 检查 HTTP/TLS/DNS-over-TCP/SSH 元数据，覆盖前置缓存、分段读取、已知域名被 SNI/Host 覆盖、每个嗅探器重读相同字节、载荷保留、绝对 deadline 与读取错误。新回归在原实现和修改后均通过竞态检测 ×20；修改后的整个 `common/sniff` 测试包也通过 `-race -count=20`。构建脚本对新回归重复执行 20 次。

复测时，将测试文件复制到独立固定源码副本的 `common/sniff/`，应用补丁前后分别运行：

```bash
GOTOOLCHAIN=go1.27.1 go test -mod=readonly ./common/sniff \
  -run '^TestMiaoSniff' -bench '^BenchmarkMiaoSniff$' -benchmem -benchtime=200ms -count=6
```

本地三目标内核构建、资源清单/定制哈希及压缩解压哈希、Linux 原生版本检查、Rust 516 项测试（另 3 项忽略）、Bun 30 项测试和 ShellCheck 均通过。未部署或重启当前实例；Windows/OpenWrt 真机及端到端网络分流没有在本轮重测。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，README 另有派生作品命名限制。定制内核使用独立名称并保留出处；对应源码由固定 SHA、本仓库补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
