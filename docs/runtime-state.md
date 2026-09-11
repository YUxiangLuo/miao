# 运行状态与配置事务

代理生命周期、订阅拉取和配置提交各有独立状态。HTTP 完成不代表配置已接受，配置写入也不能授予内核 readiness。REST 与 MCP 读取和修改同一套业务状态。

## 状态归属

| 状态 | 负责人 / 含义 |
| --- | --- |
| `running` / `pid` | 内核控制服务观察到的子进程 |
| `phase` / `ready` / `should_run` / `generation` | `state/lifecycle.rs` 的阶段、健康、运行意图和所有权快照 |
| `subscription_refresh` | `state/subscriptions.rs` 的拉取活动、结果与重试等待 |
| `sub_refresh_success_generation` | 订阅响应已被接受且完成提交的代次 |
| 订阅节点快照 | 最近一次被运行配置接受的节点材料 |

`/api/status` 与 MCP `get_status` 共用快照。旧 `RuntimePhase` 的 `fetching_subscriptions` / `refreshing_subscriptions` 只为兼容保留，新代码不再发布。`initializing` 仅控制首次资源初始化，不判断内核健康。

## 内核生命周期与控制权

锁顺序为 **`config_update → sing_process → lifecycle`**，只取必要的锁；lifecycle 锁用于短同步更新，不跨 await。网络拉取在配置锁外。

- 启动、重载、停止在进程槽锁内 `begin(KernelOperation)`，递增 generation、改变阶段并清除 readiness；停止须先使旧任务失效，再等待进程退出。
- 探测和 watchdog 携带起始 generation，成功或失败都不能覆盖新实例。发布 Ready 时，在进程槽锁内再次确认代次、运行意图和子进程存活；`running/pid/phase/ready` 由同一次观察产生。
- `RuntimeActivity` guard 仅临时显示配置校验/应用阶段，退出时恢复；若发生内核事件，旧 guard 不得恢复旧阶段，也不能授予 readiness。
- 存活但不健康的内核回滚后仍须激活并重新探测，恢复磁盘文件不足以发布就绪。
- watchdog 在取得进程槽锁后、退避结束并取得配置锁后均检查所有权；旧任务不能重启、收割新进程或覆盖/清除新告警。巡检 2 秒，连续崩溃重试最多 5 次、退避 1–16 秒，稳定运行 60 秒后清零计数。

普通停止允许再次启动；整个服务关闭先设置不可逆终止标记、取消订阅任务，再等待事务收尾并停核，拒绝晚到请求或回滚重新启核。Unix 用 SIGHUP 重载，Windows 用停止/启动，共用上述模型。

## 业务入口与配置事务

REST handler 只解析参数、调用 `services/commands/` 并由 `responses.rs` 映射响应；MCP 直接调用同一业务服务，不构造 HTTP `State/Json` 或调用 handler。业务返回值不依赖 Axum，保留两种入口的字段、提示和确认语义。破坏性 MCP 工具须保留后果说明、`destructiveHint` 与 `confirm: true` 闸。

| 操作 | 事务规则 |
| --- | --- |
| 本地节点/规则/VPS 节点编辑 | `ConfigEdit` 持锁读取最新配置并校验，丢弃未提交候选无副作用；拒绝直接修改订阅列表 |
| 订阅编辑/刷新 | 锁内登记代次 → 锁外拉取 → 锁内检查代次并合并最新输入，不覆盖并发的策略/倍率/规则编辑 |
| 策略/倍率 | `apply_preference` 先快照文件，保存 requested 值后应用候选；失败恢复内存及原文件字节（原先不存在则删除），锁覆盖回滚与 effective 返回值读取 |
| MCP 开关 | 只提交稳定配置及内存，不进入内核激活 |
| 定时刷新设置 | 只提交稳定层 `config.yaml` 及内存，不进入内核激活；Notify 唤醒调度循环重算下一次执行 |

节点切换统一经 `services/proxy` 串行校验 selector、调用 Clash PUT、保存 `.last_proxy` 并淘汰旧恢复任务，面板与 MCP 不另建切换路径。地区筛空的 effective `manual` 不覆盖 requested strategy；地区和倍率判断使用当前显示元数据，不从可能保留旧名称的稳定 tag 推导。

| 资源 | 提交 / 回滚边界 |
| --- | --- |
| active `config.json` + node bindings | `RuntimeCheckpoint` 严格快照，不可读则拒绝编辑；激活失败恢复两者，运行态恢复失败也继续尝试 bindings |
| 稳定/易变配置 + effective 配置 | `commit_generated` 在配置编辑时提交完整候选，刷新时只提交 effective selection；持久化失败回滚运行态，不发布候选诊断 |
| requested 偏好 | 由偏好事务管理独立文件和内存，不混入地区回退后的 effective 值 |
| 订阅快照、倍率选项、跳过规则、告警、运行缓存 | 接受配置后才发布；派生快照/缓存写盘为 best-effort |

候选配置先通过限时 10 秒的 `sing-box check` 再激活。安装管线的 `SubSource` 只接收本地快照或预拉取材料，没有隐式网络请求。回滚持锁且不联网，按内存快照 → `config.json.cache` → 本地订阅快照/手动节点恢复；内核已死先 `check` 再启动，拒绝空 cache。材料不足时保留可用运行态并报错，不退化为持锁拉订阅。

这是进程内失败回滚，不承诺跨文件或崩溃原子性；停止态刷新也必须报告回滚失败。自升级仅在资源释放成功且预期数据面 ready 后确认健康；空配置或用户明确停服无需就绪数据面。

生成配置保留 `route.find_process: true`，即使没有进程规则也要为面板提供进程信息。路径只使用已解析的 `AppState`，归属与临时目录生命周期见 [Profile 文档](profiles.md)。

## 订阅刷新

所有刷新共用 `refresh_subscriptions`。定时刷新到点后走与面板手动刷新相同的前台路径；调度循环只负责计时、唤醒和失败退避（1/5/15 分钟有界预算，不写入订阅刷新状态），不另建拉取或提交逻辑。退避也不会重置或加速启动恢复的重试预算。`SubscriptionFetchReport` 分开统计成功/失败来源、新鲜节点和缓存节点；计数在禁用、地区、倍率筛选前完成。

| `outcome` | 含义 |
| --- | --- |
| `not_requested` | 没有来源执行拉取 |
| `success` | 所有来源成功，有新鲜节点 |
| `empty` | 所有来源成功，没有代理节点（包括仅含账户信息节点） |
| `partial_failure` | 部分来源失败，成功来源也可能为空 |
| `failed` | 所有来源失败，即使缓存仍可用 |

`SubStatus.success` 表示响应被接受，成功空列表为 `true` 且 `node_count == 0`；失败也可保留 `node_count > 0` 的缓存。`failure_kind` 区分 network/http/timeout/parse，全部节点解析失败不视为权威空列表。`has_sub_nodes` 只描述材料可用，零网络重建的 `subscription_fetch` 为 `None`。

`subscription_refresh.phase` 为 idle / fetching / completed / failed / waiting；completed 表示至少一个来源成功，不表示已激活。waiting 的 `retry_in_secs` 是下轮剩余秒数。后台拉取和退避不修改代理 phase/ready，重试预算见[配置参考](config.md#开机订阅刷新失败)。

任务状态用短临界区更新，generation 淘汰旧操作，attempt 淘汰同代旧 fetch，旧 future 析构也不能覆盖新状态。前台操作在释放配置锁前登记 guard，直到提交/回滚结束才释放；后台重试须等事务完成。停服或订阅变更取消旧任务与等待，前台失败不重置后台快速重试额度。

成功空列表不按网络失败重试：有替代节点则提交并删除成功来源的旧节点；无可用候选但代理仍可用时，保留 PID、运行配置和已提交快照，告警后结束本次后台恢复。数据面不可用时继续恢复，但错误须描述“没有可用节点”，不能冒充订阅请求失败。地区回退不改变拉取健康度。

## 回归测试

测试覆盖陈旧启动/重载/watchdog、停止和关闭竞争、Ready 发布、空列表与缓存、前后台重试和提交边界，以及 REST/MCP 共用事务、偏好原字节恢复和双重失败报告。常规成功路径使用临时目录、假内核和 localhost 订阅；浏览器使用 mock API，不连接生产实例。命令及隔离要求见[开发检查](../DEV_NOTES.md#开发检查)。
