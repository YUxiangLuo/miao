# 运行状态与订阅刷新

代理生命周期与订阅任务是两个独立维度。后台 HTTP 拉取（包括首次、快速重试、低频重试）不修改代理 `phase` 或 `ready`；配置激活和内核健康检查仍走现有管线。

## 状态归属

| 状态 | 负责人 | 含义 |
| --- | --- | --- |
| `running` / `pid` | 内核控制服务 | 子进程是否存在 |
| `phase` / `ready` | 启动、配置激活、内核生命周期 | 代理当前工作阶段及 readiness |
| `subscription_refresh` | `state/subscriptions.rs` | HTTP 刷新活动、最近结果、重试等待 |
| `sub_refresh_success_generation` | 前台配置事务 | 已接受订阅响应且完成提交的代次，不是单纯 HTTP 完成 |
| 订阅节点快照 | 配置提交管线 | 最近一次被运行配置接受的节点材料 |

REST `/api/status` 与 MCP `get_status` 使用同一 `subscription_refresh` 快照。原有字段保留；`fetching_subscriptions` / `refreshing_subscriptions` 仍保留在旧 `RuntimePhase` 类型中以兼容已有客户端，但新代码不再发布这两个值。

本轮没有重写内核 supervisor，也没有将 `initializing`、服务期望状态和内核健康检查合并成一套大状态机。

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

## 回归测试

默认成员 Rust 测试覆盖：成功空列表提交、无替代节点时保留内核、缓存与拉取健康分离、HTTP/解析错误分类、首轮及低频拉取不改变代理阶段、陈旧任务取消、前台提交边界与重试等待。

前端测试覆盖成功空列表与重试提示；浏览器验收使用本地 mock API，不连接生产实例。所有成功启动测试使用临时目录、假内核与 localhost 订阅，不启动真实 TUN。
