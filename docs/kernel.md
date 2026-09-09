# Miao 客户端内核

Miao 使用固定上游提交的客户端构建，定制源码由本仓库追踪。普通 Miao 发版不会自动升级内核。

## 固定基线与裁剪范围

[`scripts/sing-box/source.json`](../scripts/sing-box/source.json) 是内核版本和能力清单的唯一入口，记录上游仓库、完整 SHA、Go 版本、Miao 内核版本、profile、构建标签、节点协议、DNS transports 和 TUN stack。当前 `miao-client-v2` 基于 `9ed2254c71edbef2020c23680d77e5d32aaa238b`、Go 1.25.5，保留 `with_quic,with_clash_api,with_utls`。即使本机 Go 较新，也使用固定工具链。

[`client.patch`](../scripts/sing-box/client.patch) 仅调整组件注册：

- 入站保留 TUN；去掉代理服务端入站、redirect/tproxy 等入口。
- 去掉额外服务及证书签发组件；Clash API 与缓存仍保留。
- 节点出站保留 Shadowsocks、VMess、VLESS、Trojan、AnyTLS、Hysteria2、TUIC 七种；内部 direct、selector、urltest 保留。保留这些协议现有的传输、TLS/uTLS、Reality、复用及 Shadowsocks 插件实现。
- DNS 注册仅保留 UDP、HTTPS 和 local：Miao 使用 UDP 本地 DNS 与 HTTPS 远程 DNS，local 用于上游隐式兜底。去掉 TCP/TLS/QUIC DNS、FakeIP、mDNS、DHCP 等额外注册；endpoint 注册为空。

手动 JSON 也限定为以上七种协议。旧配置里的 SOCKS、HTTP、SSH、Tor、Snell、ShadowTLS、Hysteria v1 等节点在读取和生成时会被跳过，日志显示不支持的协议及允许列表；原始 JSON 不会被自动删除。内核 `check` 同样拒绝已移除的类型。面板和订阅解析器原本支持的七种协议继续保留。

生成的 TUN 配置显式使用 `stack: "go"`。固定基线已经默认使用 GoTUN，无需新增构建标签；保留上游可选的 system 栈实现，便于需要时回退诊断，没有改写 TUN 工厂或添加自动切栈逻辑。

构建先生成未裁剪的 host `sing-box-host`，用于编译 `.srs` 规则。再应用客户端补丁，从固定上游复制 run/check/version 与 namespace 辅助实现，组装 `cmd/miao-kernel`。这样无需维护另一份重载、信号或 Windows 退出逻辑。该构建的命令名称为 `miao-kernel`，运行时文件路径仍为原来的 `sing-box` / `sing-box.exe`。

旧 context 隔离功能补丁已合入基线，回归测试独立保留；背景见[补丁历史](../scripts/patches/README.md)。

## 构建

```bash
./scripts/build-embedded.sh
MIAO_TARGET=arm64 ./scripts/build-embedded.sh
MIAO_TARGET=windows-amd64 ./scripts/build-embedded.sh

# 只验证、构建内核，不下载或改动规则
./scripts/build-embedded.sh --kernel-only
```

工具为 Git、固定 Go 工具链、Bun、curl；压缩使用 Bun 的 Zstandard API（level 19），运行时使用纯 Rust 流式解码器，无额外解压命令或 C 库要求。`SING_BOX_REF` 可以显式传入同一个固定 SHA；传入其他值会失败，避免环境变量意外升级内核。规则的 `SING_GEOIP_REF` / `DIRECT_RULES_REF` 仍可指定分支、tag 或完整 SHA，Release CI 在一次发布内统一解析规则快照。

Rust release 使用 `opt-level = "s"`、thin LTO、单 codegen unit 和符号剥离，保留 panic unwind。它缩小负责面板和配置管理的 Rust 程序，可能增加链接时间并改变控制面性能；代理数据面仍由独立 Go 内核运行。

每个目标输出：

- `embedded/sing-box-<target>`（Windows 为 `.exe`）：用于构建验证的原始内核。
- 同名 `.zst`：实际嵌入 Rust 的压缩数据。
- 同名 `.meta.json`：源码 SHA、工具链、tags、版本、能力清单、定制文件哈希、压缩前后大小和 SHA-256。

Rust 只嵌入对应目标的 `.zst` 和清单。host 内核与原始目标文件不会再嵌入成品。构建全部成功后才写入 `embedded/`；修改定制代码后仍需重建 embedded，再重新构建 Miao。

## 释放与文件完整性

每次启动在运行目录内创建临时文件，流式解压，检查解压大小、SHA-256 及完整帧，设置执行权限，再原子替换旧内核。损坏、截断、校验错误或写入失败会保留旧内核并报错；旧版本的缓存和配置仍保留。Windows 遇到运行中的文件锁继续报告残留进程错误。

压缩减少下载和可执行文件体积；运行目录里仍是解压后的普通可执行文件，不能据此推算 RSS 同比例下降。level 19 比 level 10 增加构建时压缩耗时；本轮三平台产物的解压窗口从 4 MiB 增至 8 MiB，启动时需要额外的解码历史缓冲，解压完成后释放。OpenWrt 上的启动时间和峰值内存需要在目标路由器验收。

## v2 体积记录

2026-09-09 在 Arch Linux amd64 上实测；MiB = 1,048,576 字节。v1 与 v2 使用相同上游 SHA、规则和前端，Miao 为本机原生 release 构建。

| 产物 | v1 | v2 |
| --- | ---: | ---: |
| Linux amd64 原始内核 | 31,182,996 B / 29.74 MiB | 24,080,532 B / 22.96 MiB |
| Linux amd64 压缩内核 | 10,238,896 B / 9.76 MiB | 7,385,999 B / 7.04 MiB |
| Linux amd64 完整 Miao | 27,819,128 B / 26.53 MiB | 15,875,544 B / 15.14 MiB |

完整 Miao 比 v1 再减少 42.9%。其中 Rust release 优化负责控制面程序的缩减，协议/DNS 注册裁剪与 level 19 压缩负责内嵌内核的缩减。v2 的 arm64 内核为 22,413,460 B，压缩后 6,581,921 B；Windows amd64 内核为 23,612,416 B，压缩后 7,246,800 B。这里未测量 Windows 桌面壳或 OpenWrt 完整产物体积。

## 升级步骤

1. 选择上游提交，审查配置、协议、TUN、DNS、Clash API 和工具链变更，修改 `source.json` 的 SHA、Go 和内核版本。
2. 审查并更新 `client.patch`；冲突直接处理，禁止静默跳过。上游已有等效修复时删除功能补丁，保留回归测试。
3. 执行三目标构建，运行 Rust 和脚本检查。构建会先对未修改上游运行两项隔离测试并记录出站/DNS/endpoint 支持，再验证客户端注册精确符合能力清单、属于上游支持范围且仅暴露 TUN 入站，并校验 11 组客户端配置与 8 种已移除出站的拒绝行为；每轮测试重复 20 次。
4. 在隔离环境验证 Linux/OpenWrt TUN 与 DNS 分流、Clash 面板功能和 AnyTLS 连续重载；在 Windows 真机验证 UAC、TUN、停止/退出和升级。现有生产代理不能作为随意启停的测试实例。
5. Review 后发布。Release 同时提供 `miao-embedded-sources.txt` 与各平台 `miao-kernel-*.json`，可追溯内核和压缩产物。

```bash
bun test scripts
shellcheck scripts/build-embedded.sh scripts/test-rust.sh
actionlint .github/workflows/quality.yml .github/workflows/build-release.yml
./scripts/test-rust.sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo check -p miao-core --locked --target x86_64-pc-windows-gnu
```

CI 的 Rust 测试使用本轮构建的 Linux / Windows 真实压缩内核验证释放与校验，不执行输出文件；缺失的交叉目标和规则资源用 inert stub 补齐。fresh clone 的本地测试也可只使用 stub。内核 job 在 Linux/Windows 上运行 Go 回归测试与 `version`，arm64 做交叉编译。配置测试无 inbound/TUN、无真实代理连接。上述 CI 不能替代目标平台上的实际网络验收。

## 来源与许可

上游 sing-box 使用 GPL-3.0-or-later，并在 README 中注明派生作品命名限制。定制内核使用独立名称，保留上游出处；对应源码由固定上游 SHA、本仓库裁剪补丁和构建文件共同确定。分发时保留上游许可及对应源码获取方式。
