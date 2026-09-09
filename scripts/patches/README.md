# sing-box 补丁历史

当前固定基线、裁剪补丁和独立回归测试位于 [`../sing-box/`](../sing-box/)，维护流程见[内核说明](../../docs/kernel.md)。补丁不匹配时直接停止构建，不能静默跳过。

## 已移除：`sing-box-isolate-cli-context.patch`

2026-09-09 核对固定基线 `9ed2254c71edbef2020c23680d77e5d32aaa238b`，上游 `create()` 与 `check()` 两处均已有等效修复。只加入回归测试、不应用功能补丁时，两项测试各 20 轮通过。因此移除旧功能补丁，将测试独立保留为 [`miao_context_test.go`](../sing-box/tests/miao_context_test.go)，构建时同时验证未裁剪上游与 Miao 客户端。

修复 Unix `SIGHUP` 重载期间的共享服务注册表污染。生产日志中的调用链是：

```
ReferenceManager.loop → update → applyKeepIdle
→ anytls.Outbound.SetKeepIdleConnections
→ sing-anytls.Client.SetKeepIdleConnections → nil pointer panic (exit 2)
```

CLI 的 `create()` 与 `check()` 原本都用 `context.WithCancel(globalCtx)`。它只隔离取消信号，不复制 sing 的可变服务注册表。`SIGHUP` 在旧实例仍运行时调用 `check()`；检查用的 `box.New()` 把临时 manager 写入同一注册表，旧实例的 ReferenceManager 就可能读到检查实例中未经过 `Start` 的 AnyTLS client。最高倍率变更只是触发这条重载路径，其他配置变更也可能触发。

补丁在 **create 和 check 两处**使用 `service.ExtendContext(globalCtx)` 复制注册表。既不让运行实例污染 CLI 模板，也不让检查实例覆盖运行实例；保留原有取消语义、协议注册与热重载行为，不靠空指针保护掩盖生命周期错误。

- 基于内核提交 `b4b5af50b37dbdb3015e7a4b6b1c08a2dee80dbe`；原代码的两项注册表隔离断言均失败，补丁后连续 20 轮通过。也已在上游 `60b504a1c74a33fe24872c8144c8f0b7d3d61b2a` 验证补丁应用与 20 轮测试通过。
- 独立保留的两项 `TestMiao*IsolatesServiceRegistry` 测试分别保护 create/check 边界；不并行修改 CLI 全局变量。
- 测试只用临时配置、direct 与未被选中的本地 AnyTLS outbound；无 inbound、TUN、订阅或外部连接。
- 将测试复制到固定上游后可运行：`go test -tags with_quic,with_clash_api,with_utls ./cmd/sing-box -run '^TestMiao(Check|Create)IsolatesServiceRegistry$' -count=20`。
- 后续补丁也应在上游合入等效修复后核对行为、删除功能修改、保留回归测试。不要仅因为构建冲突就跳过。

定制构建由 miao 仓库版本追踪；Go build info 中 `vcs.modified=true` 是预期现象。仅改构建文件或 Rust 重编译不会更新已运行的内核：需要重建 embedded 资源、重新嵌入 miao，再按 `DEV_NOTES.md` 的部署流程升级。
