# sing-box 构建补丁

`build-embedded.sh` 在克隆上游后应用此目录的补丁，然后用 host Go 工具链运行补丁内的回归测试。补丁不匹配时直接停止构建，不能静默跳过。

## `sing-box-isolate-cli-context.patch`

修复 Unix `SIGHUP` 重载期间的共享服务注册表污染。生产日志中的调用链是：

```
ReferenceManager.loop → update → applyKeepIdle
→ anytls.Outbound.SetKeepIdleConnections
→ sing-anytls.Client.SetKeepIdleConnections → nil pointer panic (exit 2)
```

CLI 的 `create()` 与 `check()` 原本都用 `context.WithCancel(globalCtx)`。它只隔离取消信号，不复制 sing 的可变服务注册表。`SIGHUP` 在旧实例仍运行时调用 `check()`；检查用的 `box.New()` 把临时 manager 写入同一注册表，旧实例的 ReferenceManager 就可能读到检查实例中未经过 `Start` 的 AnyTLS client。最高倍率变更只是触发这条重载路径，其他配置变更也可能触发。

补丁在 **create 和 check 两处**使用 `service.ExtendContext(globalCtx)` 复制注册表。既不让运行实例污染 CLI 模板，也不让检查实例覆盖运行实例；保留原有取消语义、协议注册与热重载行为，不靠空指针保护掩盖生命周期错误。

- 基于内核提交 `b4b5af50b37dbdb3015e7a4b6b1c08a2dee80dbe`；原代码的两项注册表隔离断言均失败，补丁后连续 20 轮通过。也已在上游 `60b504a1c74a33fe24872c8144c8f0b7d3d61b2a` 验证补丁应用与 20 轮测试通过。
- 补丁附带两项 `TestMiao*IsolatesServiceRegistry` 测试，分别保护 create/check 边界；不并行修改 CLI 全局变量。
- 测试只用临时配置、direct 与未被选中的本地 AnyTLS outbound；无 inbound、TUN、订阅或外部连接。
- 在上游源码中可重复运行：`go test -tags with_quic,with_clash_api,with_utls ./cmd/sing-box -run TestMiao -count=20`。
- 上游合入等效修复后，核对 create/check 两处及回归测试，再删除补丁和构建脚本对应步骤。不要仅因为构建冲突就跳过。

补丁由 miao 仓库版本追踪；构建产物的 Go build info 中 `vcs.modified=true` 是预期现象。仅改此补丁或 Rust 重编译不会更新已运行的内核：需要重建 embedded 资源、重新嵌入 miao，再按 `DEV_NOTES.md` 的部署流程升级。
