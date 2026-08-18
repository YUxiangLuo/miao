# 数据落盘三层重构计划

> 状态：Step 1–4 已全部完成（v0.34.0 之后）。
>
> 目标：把数据落盘按「稳定配置 / 易变配置 / 运行状态」三层拆分。
> OpenWrt 上易变层写 tmpfs（消灭切节点/切模式的闪存写），稳定层写闪存（配置持久）；
> Linux/Windows 做同样的逻辑分层，层的位置按平台语义决定。

## 一、三层定义（最终版）

```
稳定层 config.yaml     port / subs / nodes / custom_rules / mcp
                       OpenWrt+Linux: /etc/miao（低频写，可接受）
易变层 volatile.yaml   node_select / route_mode
                       OpenWrt+Linux: /tmp/miao-sing-box/（tmpfs，随系统重启清空）
                       Windows: %LOCALAPPDATA%\io.github.yuxiangluo.miao\（持久）
状态层（不动）          config.json / .cache / sub-nodes.json / .last_proxy / cache.db
```

原则：**分层全平台统一，层的位置由平台决定。**

- OpenWrt：易变层在 tmpfs → 切节点/切模式零闪存写
- Linux：易变层在 /tmp → 保住「系统重启回规则分流」的 fail-safe；
  `node_select` 重启恢复由持久的 `.last_proxy`（systemd 下落 /etc/miao）兜住 manual 节点；
  代价是 reboot 后 `fastest_*` 模式回 manual（文档化的行为变化）
- Windows：易变层持久 → `node_select` 同今天；`route_mode` 新增持久（桌面用户预期设置粘滞）

## 二、附带收益

1. **启动模式抖动消失**：改前全局模式下重启进程，秒开 cache（全局）与内存 override（规则）
   不一致 → 先全局跑几秒再重启内核切回规则。改后合并视图与 cache 字节一致 →
   `SkippedUnchanged`，不重启。
2. **进程重启保持模式**：自升级 exec / systemd restart 后 route_mode、node_select 不再丢失。
3. **消灭平行状态通道**：`route_mode_override`、`apply_runtime_config_change`、
   `persist_effective_node_select` 直写 config.yaml 全部收敛。
4. **config.yaml 里的 `route_mode` 从「被警告并忽略」变成「启动默认值」**（overlay 优先级：
   volatile > config.yaml > 默认）。

## 三、加载/保存语义

```rust
// models/config.rs 新增
pub struct VolatileConfig {           // 全 serde default；文件缺失/损坏 = 默认
    pub node_select: NodeSelect,
    pub route_mode: RouteMode,
}
impl From<&Config> for VolatileConfig { .. }
impl Config { pub fn overlay(self, v: VolatileConfig) -> Config { .. } }
```

- **加载**：config.yaml → Config（旧文件里的 node_select/route_mode 键正常反序列化
  = 免费向后兼容）→ 读 volatile 文件 → overlay → 合并视图进 AppState
- **保存**：Config 的 node_select/route_mode 改 serde 单向 `skip_serializing`
  （保留反序列化），`save_config_to` 写出的稳定层 YAML 天然不含易变字段
  （惰性迁移：首次保存自动剥离旧键）；`save_volatile_to` 写 volatile 文件。
  两者都复用 `write_file_atomic` + 内容比对跳过未变写入——一次变更两层各尝试写一次，
  没变的那层零 I/O，不需要 diff 逻辑
- volatile 路径注入 `AppState.volatile_path`（与 config_path 同款），测试指向临时目录

## 四、改动清单

| 文件 | 改动 |
|---|---|
| `models/config.rs` | 加 `VolatileConfig`；node_select/route_mode 改单向 skip；更新 serde 测试 |
| `paths.rs` | 加 `volatile_config_path()`（cfg 分叉） |
| `state.rs` | 删 `route_mode_override`；加 `volatile_path: PathBuf` |
| `services/config/persist.rs` | 加 `load_volatile_config_at`/`save_volatile_to`（`_at` 变体）；`persist_effective_node_select` 改写 volatile |
| `services/config/apply.rs` | 删 `config_with_route_override`、`apply_runtime_config_change`；apply 成功路径补写 volatile；`regenerate_preserving_service_state` 去 override 合并 |
| `services/config/mod.rs` | 导出 volatile 读写 |
| `runtime.rs` | 加载链 yaml → volatile → overlay；删 `config_declares_route_mode` 及 3 测试 |
| `handlers/service.rs` | get_status 改读 config.route_mode；set_route_mode 改走 `apply_config_change` |
| `services/mcp.rs` | tool_set_route_mode 同步 |
| `test_support.rs` | `app_state` 补 volatile_path 指向 tempdir |
| README / DEV_NOTES | 「会话级」段落重写；三层模型入档 |

**不动**：builder.rs、region.rs、generate.rs、proxy.rs、router.rs、前端、桌面壳、
所有易变字段的消费方（都吃合并视图）。

注意：`node_select` 与 `.last_proxy` 是互补不是冗余——前者是节点池策略
（manual/fastest_*），后者是 manual 模式下 selector 里的具体节点记忆。两者都保留。

## 五、测试策略

新增：
- `VolatileConfig` 序列化往返、缺省/损坏文件回落默认
- overlay 优先级：volatile > config.yaml > 默认
- 稳定层保存后 YAML 不含 node_select/route_mode（迁移断言）
- volatile 保存跳过未变写入
- set_route_mode 走 apply 管线后 volatile 落盘、state 更新

修改：
- `get_status_reports_route_mode_override_without_mutating_config` → 改为「route_mode 来自合并配置」
- `config_ignores_route_mode_when_deserializing` → 反转为「反序列化生效」
- `persist_effective_node_select_writes_manual_fallback` → 断言写 volatile 而非 config.yaml

门禁（DEV_NOTES 规定，全绿才算完成）：
```bash
cargo test --locked --all-targets
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo check -p miao-core --target x86_64-pc-windows-gnu
```
禁止：跑 `cargo run -p miao-cli`、停本机 systemd miao 实例、测试写 /tmp/miao-sing-box。

## 六、执行顺序（每步独立可验、可回滚）

1. **Step 1：volatile 基础设施**——VolatileConfig + 路径 + 读写 + overlay + state 注入 + 测试。纯增量，不改行为
2. **Step 2：node_select 迁入**——加载 overlay、persist_effective_node_select 改写、保存剥离
3. **Step 3：route_mode 迁入**——删 override/平行管线/警告-擦除，set_route_mode 并入 apply 管线
4. **Step 4：文档 + 全量门禁**

## 七、行为变化与风险

| 变化 | 平台 | 说明 |
|---|---|---|
| 切节点/切模式不再写闪存 | OpenWrt | 本重构的核心收益 |
| 系统重启后 route_mode 回规则 | OpenWrt/Linux | 与今天一致（fail-safe 保留） |
| 系统重启后 node_select 回 manual | OpenWrt/Linux | 新；`.last_proxy` 仍兜住 manual 具体节点 |
| route_mode 持久 | Windows | 新；符合桌面预期 |
| 进程重启保持 route_mode/node_select | 全平台 | 改进（自升级不再静默切回规则） |
| 旧 config.yaml 的 route_mode 生效 | 全平台 | 原被忽略；更直觉，release note 说明 |
| set_route_mode 空节点：报错 → 正常持久化（Clear 模式） | 全平台 | 更合理 |

风险：双文件无跨文件原子性——真正一致性锚点是校验过的 config.json+cache；
两层各自原子写，且 `save_config_layered` 在易变层写失败时把稳定层回写旧字节
（best-effort 补偿，故障注入测试覆盖）；补偿本身也失败时撕裂留待下次成功保存自愈。

## 八、Review 后记（已修复）

- **P1 分层落盘撕裂**：`save_config_layered` 稳定层先写、易变层后写，后者失败时
  稳定层已是新值。修复：写入前读旧字节，失败时回写/删除补偿。
  测试：`save_config_layered_rolls_back_stable_on_volatile_failure`（volatile_path
  指向目录做故障注入）、`save_config_layered_skips_unchanged_stable_layer`
  （稳定层目录只读证明未变即跳过）。
- **P2 spawn_server 测试隔离**：新增 `RuntimeOptions.volatile_path` 注入点（与
  config_path 同款）；3 个 spawn 测试传临时路径，不再读真实
  `/tmp/miao-sing-box/volatile.yaml`。
- **P3（记录在案，不修）**：启动快速通道 cache 匹配只含 node_select 不含 route_mode；
  停机手改 volatile.yaml 时会以旧模式起跑，Startup 刷新秒级自愈。
- **P4（记录在案）**：README 的「tmpfs」表述在 /tmp 为磁盘挂载的发行版上不成立，
  此时易变层实际持久，仅影响「系统重启回默认」的语义。
