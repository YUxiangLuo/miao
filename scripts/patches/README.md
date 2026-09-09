# CLI context 隔离修复

旧补丁 `sing-box-isolate-cli-context.patch` 已由上游等效实现替代。当前裁剪补丁、能力清单和测试在 [`scripts/sing-box/`](../sing-box/)，构建与升级见[内核维护](../../docs/kernel.md)。

问题发生在 Unix SIGHUP 重载：`context.WithCancel(globalCtx)` 只隔离取消信号，仍共享可变服务注册表。`check()` 创建的临时实例会覆盖运行实例的 manager，导致旧 ReferenceManager 读到尚未启动的 AnyTLS client，并在 `SetKeepIdleConnections` 中空指针崩溃；最高倍率等配置变更都可能触发。

修复要求 **create 和 check 两处**使用 `service.ExtendContext(globalCtx)` 复制注册表，同时保留取消语义。不能仅增加空指针保护，也不能只隔离检查实例。

[`miao_context_test.go`](../sing-box/tests/miao_context_test.go) 独立保护这两个边界：只使用临时配置、direct 和未选中的 AnyTLS outbound，无 inbound/TUN/外部连接；不并行修改 CLI 全局变量。构建脚本在未裁剪上游和 Miao 客户端上各重复 20 次。

后续遇到补丁冲突须审查行为；上游已有等效修复时删除功能修改，保留回归，不能跳过验证。定制源码的 Go build info 显示 `vcs.modified=true` 是预期现象。
