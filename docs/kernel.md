# Miao 客户端内核

Miao 使用固定上游提交的客户端构建，定制源码由本仓库追踪。普通 Miao 发版不会自动升级内核。

## 固定基线与裁剪范围

[`scripts/sing-box/source.json`](../scripts/sing-box/source.json) 是内核版本的唯一入口，记录上游仓库、完整 SHA、Go 版本、Miao 内核版本、profile 和构建标签。当前基于 `9ed2254c71edbef2020c23680d77e5d32aaa238b`、Go 1.25.5，保留 `with_quic,with_clash_api,with_utls`。即使本机 Go 较新，也使用固定工具链。

[`client.patch`](../scripts/sing-box/client.patch) 仅调整组件注册：

- 入站保留 TUN；去掉代理服务端入站、redirect/tproxy 等入口。
- 去掉额外服务及证书签发组件；Clash API 与缓存仍保留。
- 出站、DNS、endpoint 的注册保持与相同标签的未修改上游一致，包括原有功能未编入时的报错 stub。手动 JSON 节点的协议范围保持不变。

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

工具为 Git、固定 Go 工具链、Bun、curl；压缩使用 Bun 的 Zstandard API，运行时使用纯 Rust 流式解码器，无额外解压命令或 C 库要求。`SING_BOX_REF` 可以显式传入同一个固定 SHA；传入其他值会失败，避免环境变量意外升级内核。规则的 `SING_GEOIP_REF` / `DIRECT_RULES_REF` 仍可指定分支、tag 或完整 SHA，Release CI 在一次发布内统一解析规则快照。

每个目标输出：

- `embedded/sing-box-<target>`（Windows 为 `.exe`）：用于构建验证的原始内核。
- 同名 `.zst`：实际嵌入 Rust 的压缩数据。
- 同名 `.meta.json`：源码 SHA、工具链、tags、版本、定制文件哈希、压缩前后大小和 SHA-256。

Rust 只嵌入对应目标的 `.zst` 和清单。host 内核与原始目标文件不会再嵌入成品。构建全部成功后才写入 `embedded/`；修改定制代码后仍需重建 embedded，再重新构建 Miao。

## 释放与文件完整性

每次启动在运行目录内创建临时文件，流式解压，检查解压大小、SHA-256 及完整帧，设置执行权限，再原子替换旧内核。损坏、截断、校验错误或写入失败会保留旧内核并报错；旧版本的缓存和配置仍保留。Windows 遇到运行中的文件锁继续报告残留进程错误。

压缩减少下载和可执行文件体积；运行目录里仍是解压后的普通可执行文件，不能据此推算 RSS 同比例下降。OpenWrt 上的启动时间和峰值内存需要在目标路由器验收。

## 升级步骤

1. 选择上游提交，审查配置、协议、TUN、DNS、Clash API 和工具链变更，修改 `source.json` 的 SHA、Go 和内核版本。
2. 审查并更新 `client.patch`；冲突直接处理，禁止静默跳过。上游已有等效修复时删除功能补丁，保留回归测试。
3. 执行三目标构建，运行 Rust 和脚本检查。构建会先对未修改上游运行两项隔离测试并记录出站/DNS/endpoint 支持，再验证客户端注册完全一致、仅暴露 TUN 入站，并校验 13 组客户端配置。
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
