# CLI context 隔离回归

旧 `sing-box-isolate-cli-context.patch` 已由上游替代，仅保留 [miao_context_test.go](../sing-box/tests/miao_context_test.go)。当前补丁与能力清单在 [scripts/sing-box](../sing-box/)，流程见[内核维护](../../docs/kernel.md)。

Unix SIGHUP 重载时，`context.WithCancel(globalCtx)` 只隔离取消信号，仍共享可变服务注册表；`check()` 临时实例会覆盖运行实例的 manager，导致旧 ReferenceManager 访问未启动的 AnyTLS client，在 `SetKeepIdleConnections` 崩溃。

**create 和 check 两处**均须用 `service.ExtendContext(globalCtx)` 复制注册表并保留取消语义，单纯判空或只隔离 check 不足。回归分别保护两个边界，用临时配置、direct 和未选中的 AnyTLS，无 inbound/TUN/外部连接；不并行修改 CLI 全局变量。上游与客户端构建各重复 20 次。

补丁冲突须审查行为，上游等效修复后删除功能修改、保留回归。定制源码的 Go build info 显示 `vcs.modified=true` 属正常现象。
