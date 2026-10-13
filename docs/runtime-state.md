# 运行状态与配置事务

REST 与 MCP 共用业务状态。内核生命周期、订阅拉取、配置提交分别建模：请求完成不代表配置已提交，写盘成功也不代表内核就绪。

## 状态归属

| 状态 | 含义 |
| --- | --- |
| `running` / `pid` | 观察到的内核子进程 |
| `phase` / `ready` / `should_run` / `generation` | [lifecycle](../crates/miao-core/src/state/lifecycle.rs) 管理的阶段、健康、运行意图、操作代次 |
| `subscription_refresh` | [subscriptions](../crates/miao-core/src/state/subscriptions.rs) 管理的拉取活动、结果、等待 |
| `sub_refresh_success_generation` / 节点快照 | 已接受响应且完成提交的代次 / 节点材料 |

`/api/status` 与 MCP `get_status` 共用快照。`initializing` 仅表示首次资源初始化；旧阶段 `fetching_subscriptions` / `refreshing_subscriptions` 只兼容保留，不再发布。

## 内核生命周期与控制权

锁顺序：**`config_update → sing_process → lifecycle`**。只取必要的锁，lifecycle 不跨 await，网络拉取在配置锁外。

- 启动/重载/停止在进程槽锁内 `begin(KernelOperation)`，递增 generation、改阶段、清 readiness；停止先使旧任务失效，再等进程退出。
- 探测和 watchdog 携带 generation，不能覆盖新实例；发布 Ready 时在进程槽锁内复核代次、运行意图和子进程存活，`running/pid/phase/ready` 来自同一次观察。
- `RuntimeActivity` guard 只临时显示校验/应用阶段，内核事件发生后旧 guard 不得恢复旧阶段，也不得授予 readiness。回滚后存活但不健康的内核仍须激活、探测。
- watchdog 取得进程槽锁、退避后取得配置锁时都复核所有权，旧任务不得重启/收割新进程或改新告警。巡检 2 秒，连续崩溃最多重试 5 次，退避 1–16 秒，稳定 60 秒清零。

普通停止允许再启动；服务关闭先设置不可逆终止标记、取消订阅任务，再等事务收尾、停核，拒绝晚到请求或回滚重新启核。Unix 用 SIGHUP 重载，Windows 用停止/启动。

## 业务入口与配置事务

REST handler 只解析参数、调用 `services/commands/`，由 `responses.rs` 映射响应；MCP 直接调用同一服务。业务返回值不依赖 Axum，不从业务层调用 handler。破坏性 MCP 工具保留后果说明、`destructiveHint`、`confirm: true`。

| 操作 | 提交规则 |
| --- | --- |
| 节点/规则/VPS 节点 | `ConfigEdit` 持锁读取最新配置并校验；丢弃候选无副作用，不允许直接改订阅列表 |
| 订阅编辑/刷新 | 锁内登记代次 → 锁外拉取 → 锁内核对代次、合并最新输入，保留并发的策略/倍率/规则修改 |
| 策略/倍率 | `apply_preference` 快照文件、保存 requested 值、应用候选；失败恢复内存和原字节（原文件不存在则删除），锁覆盖回滚及 effective 返回值读取 |
| MCP / 定时刷新设置 | 只提交稳定配置与内存，不激活内核；定时设置用 Notify 唤醒调度重算 |

节点切换统一走 `services/proxy`：串行校验 selector、Clash PUT、保存 `.last_proxy`、淘汰旧恢复任务。地区回退的 effective `manual` 不覆盖 requested 策略；地区/倍率取当前显示元数据，不取可能保留旧名称的稳定 tag。

| 资源 | 提交与回滚边界 |
| --- | --- |
| active `config.json`、node bindings | `RuntimeCheckpoint` 严格快照，不可读则拒绝编辑；激活失败恢复两者，运行态恢复失败仍尝试 bindings |
| 稳定/易变/effective 配置 | `commit_generated` 编辑时提交完整候选，刷新时只提交 effective selection；持久化失败回滚运行态，不发布候选诊断 |
| requested 偏好 | 独立文件与内存事务，不混入地区回退值 |
| 订阅快照、倍率选项、跳过规则、告警、运行缓存 | 接受配置后才发布；派生快照/缓存写盘 best-effort |

候选先通过限时 10 秒的 `sing-box check` 再激活；`SubSource` 只接收本地快照/预拉取材料。回滚持锁但不联网，依次用内存快照、`config.json.cache`、本地订阅快照/手动节点；内核已死先 check 再启动，拒绝空 cache。材料不足保留可用运行态并报错，不持锁拉订阅。

仅保证进程内失败回滚，不承诺跨文件或崩溃原子性；停止态刷新也须报告回滚失败。自升级在资源释放成功、预期数据面 ready 后才确认健康；空配置或用户明确停服无需数据面就绪。

生成配置始终保留 `route.find_process: true` 供面板显示进程。路径取自已解析的 `AppState`，见 [Profile](profiles.md)。

## 订阅刷新

所有刷新共用 `refresh_subscriptions`。调度复用手动刷新路径，只负责计时、唤醒、退避，不另建拉取/提交逻辑，不写订阅刷新状态或改变启动重试预算；时间预算见[配置参考](config.md#定时刷新订阅)。`SubscriptionFetchReport` 在禁用/地区/倍率筛选前统计成功/失败来源及新鲜/缓存节点。

| `outcome` | 含义 |
| --- | --- |
| `not_requested` | 没有来源执行拉取 |
| `success` | 全部成功，有新鲜节点 |
| `empty` | 全部成功，无代理节点（含仅有账户信息节点） |
| `partial_failure` | 部分失败，成功来源也可能为空 |
| `failed` | 全部失败，即使缓存可用 |

`SubStatus.success` 表示响应被接受，成功空列表为 true、`node_count == 0`；失败可保留缓存和非零计数。`failure_kind` 区分 network/http/timeout/parse，全部节点解析失败不当作权威空列表。`has_sub_nodes` 只表示材料可用，零网络重建的 `subscription_fetch` 为 None。

`subscription_refresh.phase` 为 idle/fetching/completed/failed/waiting；completed 表示至少一个来源成功，不代表激活，waiting 的 `retry_in_secs` 表示下轮剩余秒数。后台拉取/退避不改代理 phase/ready。

generation 淘汰旧操作，attempt 淘汰同代旧 fetch，旧 future 析构不得覆盖新状态。前台操作释放配置锁前登记 guard，提交/回滚结束才释放，后台重试须等事务结束；停服或订阅变更取消旧任务，前台失败不重置后台快速重试额度。

成功空列表有替代节点时提交并删除旧节点；无候选但代理可用时保留 PID、运行配置和已提交快照，告警后结束本次后台恢复。数据面不可用则继续恢复，错误应为“没有可用节点”；地区回退不改变拉取健康度。

## 回归测试

覆盖旧启动/重载/watchdog、停止/关闭竞争、Ready 发布、空列表/缓存、前后台重试、提交边界、REST/MCP 一致性、偏好原字节恢复及双重失败报告。隔离要求与命令见[开发检查](../DEV_NOTES.md#开发检查)，浏览器使用 mock API。
