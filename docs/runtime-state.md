# 运行状态与订阅刷新

代理生命周期与订阅任务是两个独立维度。后台 HTTP 拉取（包括首次、快速重试、低频重试）不修改代理 `phase` 或 `ready`；配置激活和内核健康检查仍走现有管线。

## 状态归属

| 状态 | 负责人 | 含义 |
| --- | --- | --- |
| `running` / `pid` | 内核控制服务 | 子进程是否存在 |
| `phase` / `ready` / `should_run` / `generation` | `state/lifecycle.rs` | 同一临界区内的生命周期快照；阶段、健康、运行意图和内核所有权一起读取 |
| `subscription_refresh` | `state/subscriptions.rs` | HTTP 刷新活动、最近结果、重试等待 |
| `sub_refresh_success_generation` | 前台配置事务 | 已接受订阅响应且完成提交的代次，不是单纯 HTTP 完成 |
| 订阅节点快照 | 配置提交管线 | 最近一次被运行配置接受的节点材料 |

REST `/api/status` 与 MCP `get_status` 使用同一 `subscription_refresh` 快照。原有字段保留；`fetching_subscriptions` / `refreshing_subscriptions` 仍保留在旧 `RuntimePhase` 类型中以兼容已有客户端，但新代码不再发布这两个值。

## 内核生命周期与控制权

`RuntimeLifecycle` 取代分散的 `runtime_ready`、`runtime_phase`、`service_should_run`、`sing_generation` 原子字段。`initializing` 仍是首次内嵌资源/本地初始化的入口闸，不承担内核健康判断；订阅刷新也保持独立。

- 启动、重载、停止在进程槽锁内调用 `begin(KernelOperation)`，递增 generation，同时改变阶段并清除 readiness。停止在等待子进程退出**之前**使旧任务失效。
- 启动/重载探测及 watchdog 回调必须携带最初的 generation；旧回调的成功与失败都不能覆盖新实例或已停止状态。
- `publish_kernel_ready` 在进程槽锁内再次确认当前代次、运行意图和子进程存活，才发布 `Ready`。REST/MCP 的 `running`、`pid`、`phase`、`ready` 来自这一锁边界内的同一观察，而不是分别读取不同时间的原子字段。
- 配置校验/应用用 `RuntimeActivity` RAII guard 临时显示阶段，保留已有健康状态；返回、报错或取消时自动恢复原阶段。如果期间出现内核事件，旧 guard 不得恢复旧阶段。配置事务本身不能授予 readiness。
- 回滚遇到存活但不健康的内核时，必须重新激活和探测；单纯恢复磁盘文件不能把它变成就绪。
- watchdog 获取进程槽锁后再次检查 generation；退避结束后还要持 `config_update` 检查所有权。旧任务既不能收割新进程，也不能重启旧配置、覆盖或清除新任务的告警。

锁顺序为 `config_update → sing_process → lifecycle`（只取必要的锁，但保持此顺序）。生命周期锁仅用于短同步更新，不跨越任何 await；HTTP 拉取继续在配置锁外。正常服务启停和配置激活遵守配置事务顺序，内核探测/观察者则以 generation 保证晚到结果无效。

普通停止保留重新启动能力；整个服务关闭时，先设置终止标记、取消订阅请求，再等配置事务收尾并停止内核。终止标记拒绝后续启动意图，防止尚在排空的请求或回滚重新拉起内核。Windows 仍使用停止/启动替代 Unix SIGHUP，共用同一生命周期模型。

不改动已有 API 字段、TUN 参数、重启退避与次数上限，也没有引入新的 actor 框架。

## 拉取结果与可用节点

`SubscriptionFetchReport` 单独统计成功来源、失败来源、新鲜节点、失败来源保留的缓存节点。新鲜节点数在禁用、地区、倍率筛选前统计。

| `outcome` | 含义 |
| --- | --- |
| `not_requested` | 没有来源执行拉取 |
| `success` | 所有来源成功，存在新鲜节点 |
| `empty` | 所有来源成功，但没有代理节点（包括仅含账户信息节点） |
| `partial_failure` | 部分来源成功、部分失败；成功来源也可能返回空列表 |
| `failed` | 所有来源失败，即使缓存仍可用也不算成功 |

`GenConfigOutcome.has_sub_nodes` 只表示筛选前订阅节点材料可用（包括缓存），不再兼任网络健康标记。零网络重建的 `subscription_fetch` 为 `None`。

每条 `SubStatus.success` 表示响应被接受，成功空列表也是 `true`，通过 `node_count == 0` 区分。失败来源可以是 `success == false` 且 `node_count > 0`（保留缓存）。`failure_kind` 区分 `network`、`http`、`timeout`、`parse`，不依赖错误文案；节点全部解析失败不是权威空列表。

## 任务生命周期

`subscription_refresh.phase`：

- `idle`：未执行，或当前 fetch 被取消。
- `fetching`：网络请求或同一轮预算内退避。
- `completed`：至少有成功来源，包括成功空列表；不代表候选配置已激活。
- `failed`：本轮所有来源失败。
- `waiting`：后台调度器已安排下一轮，`retry_in_secs` 是剩余秒数。

任务状态由短临界区管理，网络等待不持状态锁或配置事务锁。generation 淘汰旧操作，attempt 淘汰同一 generation 内的旧 fetch 状态；取消旧 future 的析构也不能覆盖新状态。

前台订阅操作在释放 `config_update` 前登记 guard，直到提交/回滚结束才释放。后台重试到期后必须等待这次前台事务完成，而不是看到 HTTP 返回就启动竞争请求。停止服务/订阅变更取消旧代次的任务与等待时间；失败的前台请求不重置后台快速重试额度。

## 空列表与可用性保护

成功空列表不按网络失败重试：

- 有手动节点或其他可用材料：生成并提交新配置，成功来源的旧节点被删除，空快照也正常发布。
- 当前代理可用，但候选解析/筛选后无可用节点：保留运行配置、PID 和已提交快照，提示检查内容/禁用设置，结束这次后台恢复，不发布未激活的空快照。用户可手动刷新。
- 数据面本身不可用：仍保留启动恢复机制，错误应描述“没有可用节点”，不能断言订阅请求失败。

地区无候选仍按原规则回退手动，保留 requested strategy；它不改变订阅拉取健康度。

## Profile 所有权

配置及运行文件的路径由 `profile::ResolvedProfile` 在启动时确定；命名配置不再共享默认 Profile 的偏好、易变层和缓存。临时 `--sub` 的目录所有权交给 `AppState`，后台任务释放最后一个引用之前不会清理。默认兼容、绑定迁移、SDK 覆盖及 Windows/OpenWrt 差异见 [Profile 与路径归属](profiles.md)。

## 业务入口与配置事务

REST handler 只解析 HTTP 参数、调用服务并包装响应。`services/commands/` 承接节点、订阅、规则、启停和 MCP 开关等业务操作，返回不依赖 Axum 的 `CommandReply` / `CommandError`；`responses.rs` 集中映射 HTTP 状态码。MCP 直接调用这些操作（策略/倍率复用同一配置服务），不再构造 `State/Json` 或调用 HTTP handler。两种协议的原有字段、提示、确认闸保持不变。

- **本地编辑**：`ConfigEdit` 持有 `config_update`，锁内读取最新配置、校验依赖当前节点/规则的条件，再提交候选。丢弃未提交候选没有副作用。它拒绝修改订阅列表；安装管线的 `SubSource` 只有本地快照与预拉取材料，没有隐式触网分支。
- **订阅编辑/刷新**：`edit_subscriptions` 锁内登记代次 → 锁外拉取 → 锁内检查代次并合并最新输入；策略、倍率、规则等并发本地编辑不会被旧订阅快照覆盖。
- **请求偏好**：策略与倍率共用 `apply_preference`。先读取偏好文件快照，再保存并暂存 requested 值、应用候选；失败恢复内存偏好和原文件字节（原先不存在则删除）。锁覆盖回滚及读取 effective 返回值。地区回退仍不覆盖 requested。
- **MCP 开关**：属于仅持久化输入的操作，只写稳定配置及内存，不进入内核激活管线。

提交资源的边界：

| 资源 | 提交/回滚责任 |
| --- | --- |
| active `config.json` + node bindings | 生成前由 `RuntimeCheckpoint` 保存；任一文件不可读则拒绝编辑。候选校验后安装；失败同时恢复运行态和 bindings，运行态恢复失败也仍尝试恢复 bindings |
| 稳定/易变配置 + effective 配置 | `commit_generated` 的配置编辑范围提交完整候选；刷新范围只提交 effective selection。持久化失败回滚运行态，不能发布候选诊断 |
| requested 偏好 | 独立文件及内存值由同一偏好事务负责，不混入地区回退后的 effective 值 |
| 订阅节点快照、倍率选项、跳过规则、告警/运行缓存 | 接受配置后发布；派生快照/缓存写盘仍为 best-effort，不冒充严格持久化成功 |

这是进程内的失败回滚协议，不是跨文件或进程崩溃意义上的原子事务。启动恢复仍保持可用性优先；配置事务不授予内核 readiness。停止态刷新也必须上报回滚失败，不能吞掉磁盘恢复错误。

## 回归测试

默认成员 Rust 测试覆盖：成功空列表提交、无替代节点时保留内核、缓存与拉取健康分离、HTTP/解析错误分类、首轮及低频拉取不改变代理阶段、陈旧任务取消、前台提交边界与重试等待。

内核回归另外覆盖：启动/重载期间停止、旧 watchdog 等待进程槽锁、退避期间修改配置、不健康进程的回滚必须重新探测、退出进程拒绝 Ready 发布、陈旧告警清理、配置活动不能覆盖崩溃状态，以及关闭期间晚到启动请求。

事务与入口测试另外覆盖：REST/MCP 的错误及读取模型兼容、仅持久化开关、两种入口并发添加节点、丢弃候选、禁止持锁改订阅、不可读回滚材料、两种偏好的原字节/不存在状态恢复，以及停止态的双重失败报告。

前端测试覆盖成功空列表与重试提示；浏览器验收使用本地 mock API，不连接生产实例。所有成功启动测试使用临时目录、假内核与 localhost 订阅，不启动真实 TUN。
