# 运行状态与配置事务

REST 与 MCP 共用同一业务状态层。内核生命周期、订阅拉取与配置提交分别建模：**请求返回不代表配置已提交，写盘成功也不代表内核已就绪**。

## 状态归属

| 状态 | 归属与语义 |
| --- | --- |
| `running` / `pid` | 观察到的 `sing-box` 子进程状态 |
| `phase` / `ready` / `should_run` / `generation` | [lifecycle](../crates/harbor-core/src/state/lifecycle.rs) 管理的运行阶段、数据面健康、运行意图与操作代次 |
| `subscription_refresh` | [subscriptions](../crates/harbor-core/src/state/subscriptions.rs) 管理的拉取活动、结果与等待计时 |
| `sub_refresh_success_generation` / 节点快照 | 已接受响应并完成提交的代次与本地节点材料 |

`/api/status` 与 MCP `get_status` 共用同一投影；`initializing` 仅表示首次启动资源初始化。

## 锁序、内核生命周期与关机

严格锁序：**`config_update → sing_process → lifecycle`**。`lifecycle` 不跨 `.await`，网络拉取一律在 `config_update` 锁外执行。

- **代次与激活回执**：启动/重载/停止均在进程槽锁内调用 `begin(KernelOperation)`，递增 `generation`、切阶段、清 `ready`。新配置必须收到匹配唯一请求 ID、实际字节 SHA-256 与子进程 PID 的[激活回执](kernel.md#补丁与回归)，并通过 TUN/Clash 就绪探测后，才在进程槽锁内复核代次与存活并发布 `Ready`。
- **Guard 与 Watchdog**：`RuntimeActivity` guard 仅作临时阶段展示，不得覆盖后续内核事件或授予 `ready`。Watchdog 每 2 秒巡检，携带 `generation` 并在取锁后复核所有权；连续崩溃最多重启 5 次（退避 1–16 秒，稳定 60 秒清零）。
- **热重载与停止**：Unix 用 SIGHUP 热重载；Windows 走停止 → 原子写盘 → 重启并保留激活回执与失败回滚。
- **优雅关机与自升级排空**：
  - 关机先置不可逆终止标记、取消订阅后台任务并拒绝新写请求（HTTP 503），等已登记写任务收尾后再持 `config_update` 停核；回滚不得重新启核。
  - 自升级校验完成后进入可撤销静默期，拒收新写并排空存量任务后停核、替换并 `exec`（安装失败恢复准入；关机优先于交接并恢复备份）。新进程仅在匹配当前版本/PID 的 `booting` 标记下，待数据面 `Ready`（或空配置/显式停服在锁内确认）后才清理备份。

## 业务入口与配置事务

REST handler 只做参数解析并调用 `services/commands/`（经 `responses.rs` 映射），MCP 直接复用同一命令层。

**断连不取消写事务**：所有非安全 HTTP 方法（含 `POST /mcp`）在中间件完整收完请求体（默认上限 2 MiB、限时 30 秒，超时 408，关机取消返 503）后登记 mutation guard 并在独立任务中执行，避免热重载切网导致连接断开时半途取消事务。只读请求与 WebSocket 仍随断连取消。

| 操作类别 | 事务与提交规则 |
| --- | --- |
| 节点 / 规则 / 独立端口 / VPS | `ConfigEdit` 持锁读取最新配置校验；丢弃候选无副作用，禁止直接改订阅列表 |
| 工位终端（`terminals`） | 修改备注仅写稳定层 `config.yaml`，不触发内核重载；`POST /api/terminals/assign` 单事务原子更新 `/32` 出口规则 |
| 订阅编辑 / 刷新 | 锁内登记代次 → 删除优先筛选已接受快照，其余锁外拉取 → 锁内核对代次、合并最新配置（保留并发的策略/倍率/规则修改） |
| 策略 / 倍率偏好 | `apply_preference` 快照偏好文件、写 requested 值并应用候选；失败按原字节恢复文件（原缺失则删）与内存 |
| MCP / 定时刷新设置 | 仅提交稳定层与内存，不激活内核；定时变更经 `Notify` 唤醒调度器重算 |
| 手动切换节点 | 统一经 `services/proxy`：校验 selector、调用 Clash PUT、写 `.last_proxy` 并淘汰旧恢复任务 |

**提交与回滚边界**：

- **严格检查点**：active `config.json` 与 `node-bindings.json` 由 `RuntimeCheckpoint` 严格快照（不可读即拒编辑）；激活失败恢复两者，持久化失败回滚运行态且不发布候选诊断。
- **候选生成与校验**：先剔除 `detour` 前置缺失的节点及其依赖链，将引用缺失节点的自定义规则与 `access_ports`（路由 + DNS）原位改写为 `reject`，再对剩余配置执行限时 10 秒的 `sing-box check`。
- **回滚与缓存契约**：回滚持锁但不联网，依次尝试内存快照、`config.json.cache`（须匹配 `RUNTIME_CONFIG_SCHEMA_VERSION = 6` 清单与输入指纹）、本地订阅快照/手动节点。已接受订阅快照始终先更新内存；`node-bindings.json` 携带快照指纹，重启拒绝失配旧快照。仅保证进程内失败回滚，不承诺崩溃跨文件原子性。
- 生成配置始终开启 `route.find_process: true` 供面板展示进程；路径一律取自 `AppState`（见 [Profile](profiles.md)）。

## 订阅刷新与恢复

全部刷新（启动、手动、定时）共用 `refresh_subscriptions`；定时调度仅负责计时、唤醒与退避（不改订阅刷新状态或启动重试预算）。

| `outcome` | 语义 |
| --- | --- |
| `not_requested` | 本次未执行网络拉取（如基于快照删除订阅） |
| `success` | 全部来源成功且获得新鲜节点 |
| `empty` | 全部来源成功但无可用代理节点（含仅账户信息条目） |
| `partial_failure` | 部分失败（成功来源提交，失败来源沿用缓存） |
| `failed` | 全部失败（即使缓存可用仍标失败） |

- **拉取与状态投影**：`SubStatus.success` 表示响应被接受（空列表为 `true`、`node_count == 0`）；`failure_kind` 区分 `network`/`http`/`timeout`/`parse`（全解析失败不当空列表）。`subscription_refresh.phase` 为 `idle`/`fetching`/`completed`/`failed`/`waiting`，后台拉取与退避不改数据面 `phase`/`ready`。
- **后台恢复协调（`SubscriptionRecovery`）**：同一时刻仅一个后台 owner，所有权与尝试锁先于 `config_update` 获取；退避等待期间不占 mutation lease（不阻塞升级排空）。`generation` 淘汰旧操作，`attempt` 淘汰同代旧投影；停服、订阅变更或前台成功提交取消旧恢复并收敛残留 `refreshing` 状态。
- **空列表与无节点等待**：成功空列表有替代节点时提交并删旧节点；无候选但数据面可用时保留运行态并告警；数据面不可用且无可用本地缓存时，告警并直接进入 30 分钟低频等待（不触发启动快速重试），出现有效节点后自动启核。
- **显式删除订阅**：先按来源归属筛选已接受快照，仅在快照缺失、来源不全或禁用/`detour` 裁剪后无可用节点时才联网拉取剩余订阅。确认剩余来源无可用节点时，先将运行文件与缓存暂存出启动路径，再写新指纹并提交配置，成功后停核清理暂存（任一步失败回滚并保留原运行进程）。详见[订阅管理](config.md#订阅管理)。

## 回归测试

覆盖旧启动/重载/watchdog 竞争、停服/关机/升级排空、Ready 发布、空列表与缓存清单、前后台重试、提交与偏好原字节回滚、REST/MCP 一致性及双重失败报告。执行命令见[测试隔离与维护约定](development.md#测试隔离与维护约定)。
