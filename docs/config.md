# 配置参考

不创建任何文件也能用。查找顺序：`--config` → 可执行文件同目录 `config.yaml` → 平台默认路径。

```yaml
port: 6161                 # 面板端口

subs:                      # Clash YAML 订阅
  - "https://your-subscription-url"

nodes:                     # 手动节点（sing-box outbound JSON）
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'

custom_rules:              # 可选：优先于内置分流，全局模式下仍生效
  - '{"domain_suffix":"example.com","action":"route","outbound":"direct"}'
  # 进程级指定出口：让 qbittorrent 的流量固定走「香港节点」（Windows 写 qbittorrent.exe）
  - '{"process_name":"qbittorrent","action":"route","outbound":"香港节点"}'
  # outbound 除 proxy/direct 外也可填节点 tag；节点消失时该规则被跳过并在面板标记

mcp: true                  # 可选：MCP 端点（POST /mcp），默认关闭

node_select: fastest_jp    # 可选：启动默认节点策略（manual / fastest_hk/jp/tw/sg/us）
max_multiplier: 2.5       # 可选：节点最高倍率；null 或省略表示不限
route_mode: global         # 可选：启动默认路由模式（rule 规则分流 / global 全局代理）
```

**数据按三层落盘**：

| 层 | 文件 | 内容 | 位置 |
| --- | --- | --- | --- |
| 稳定层 | `config.yaml` | 订阅/节点/规则等低频配置 | Linux/OpenWrt：`/etc/miao`；Windows：应用数据目录 |
| 易变层 | `volatile.yaml` | 节点选择策略、最高倍率、路由模式、禁用的订阅节点 | Unix：`/tmp/miao-sing-box`（tmpfs，系统重启后回到 config.yaml 的启动默认值）；Windows：应用数据目录（持久） |
| 状态层 | `config.json` / `.cache` / 快照 | 运行时配置与缓存 | sing-box 运行目录，可删 |
| 选择偏好 | `.node_select` / `.max_multiplier` / `.last_proxy` | 用户显式选择的策略 / 最高倍率 / 手动节点 | 普通 Linux：`/etc/miao`；OpenWrt：运行时 tmpfs；Windows：应用数据目录 |

面板或 MCP 显式选择 `manual` / `fastest_*` 后会更新 `.node_select`，设置最高倍率后会更新 `.max_multiplier`（`unlimited` 表示不限）。普通 Linux 和 Windows 重启后优先恢复这些偏好；启动期间因地区节点暂缺而临时回退到 `manual` 不会覆盖用户偏好，后续订阅刷新会继续尝试原策略。首次升级会迁移旧 `volatile.yaml` 中明确记录的最高倍率和 `fastest_*`；无法与临时回退区分的 `manual` 不会被提升为持久偏好。具体手动节点仍由 `.last_proxy` 独立恢复。

最高倍率从节点当前显示名动态识别，例如 `18x`、`6.5X`、`2.4倍`、`倍率：1.3`；未标倍率的节点按 `1x`，明确带倍率标记但数值无效的节点不会进入受限的自动候选。该限制仅在“地区最快”自动选择模式下生效，只缩小 `urltest` 的测速候选；订阅节点、手动节点及其真实 outbound 始终保留，手动选择模式展示完整节点池。面板下拉选项来自当前完整节点池，选择“不限”可恢复全部自动候选；地区筛空而临时回退手动模式时仍可调整倍率。

OpenWrt 的易变层和选择偏好写 tmpfs：切节点/切模式这类高频操作零闪存磨损。面板/进程重启（如自升级）期间文件仍保留，系统重启后则回到 `config.yaml` 的启动默认值。

### 禁用订阅节点

面板「订阅管理」里获取成功的订阅，其「N 个节点」可点开订阅详情弹窗，逐节点禁用/启用。禁用的节点不会出现在生成的 sing-box 配置中（selector/urltest 成员、地区分组同步缩小），自定义规则若引用被禁节点会被跳过并在面板标记。禁用集是易变层配置 `disabled_nodes`：

```yaml
# volatile.yaml（由面板维护，不建议手编）
disabled_nodes:
  - sub: "https://your-subscription-url"   # 订阅 URL，与 config.yaml 的 subs 条目一致
    name: "香港 01"                          # 节点名；订阅内同名节点会一起禁用
```

语义说明：按「订阅 + 节点名」标识，订阅刷新后节点改名则旧条目失配自然失效（节点重新出现）；Unix 上易变层在 tmpfs，系统重启后禁用集清空；不允许禁用后节点池为空（含手动节点），会被 400 拒绝。

## MCP：让 AI agent 操作代理

配置里加一行 `mcp: true`（默认关闭），端点是 `http://<面板地址>/mcp`。它实现 MCP `2025-11-25` Streamable HTTP：客户端先发送 `initialize`，收到响应后发送 `notifications/initialized`，后续请求携带 `MCP-Protocol-Version`。服务端使用无 session 的 JSON 响应，不提供 SSE，因而 `GET /mcp` 返回 405。面板右下角的浮动控件可以一键开关并复制地址。

MCP 尽量与面板能力同构，工具按用途分为：

- 状态与诊断：`get_status`、`get_version_info`、`test_connectivity`、`get_traffic`（实时速率快照）、`list_connections`（支持分页）
- 服务与路由：`start_service`、`stop_service`、`set_route_mode`、`set_node_select`、`set_max_multiplier`、`switch_node`、`test_delay`
- 订阅：`list_subscriptions`、`add_subscriptions`、`delete_subscription`、`refresh_subscriptions`、`scan_clash_verge`、`list_subscription_nodes`（按订阅列出节点及禁用状态）、`set_subscription_node_disabled`（禁用/启用订阅节点）
- 节点：`list_nodes`（订阅 + 手动平铺池）、`list_manual_nodes`、`add_node`、`import_nodes`、`delete_node`
- 规则：`list_rules`、`add_rule`、`delete_rule`
- 管理：`set_mcp_enabled`、`deploy_vps`、`upgrade_miao`（平台不支持时返回明确错误）

主题切换、弹窗和 PWA 安装属于浏览器本地 UI 状态，没有服务端语义，因此不暴露为 MCP 工具。分享链接解析也保留在浏览器端；MCP 调用者可自行解析后交给结构化的 `add_node` / `import_nodes`。节点/订阅/规则写操作复用面板 HTTP handler，不另造一套配置逻辑。

连接时服务端通过 `instructions` 告知调用者：流量很可能正经过本代理，配置热应用可能影响连接；订阅 URL、连接记录和 VPS 密码属于敏感信息。停止服务、删除配置、部署 VPS、关闭 MCP、升级 Miao 等破坏性工具既在描述和 `annotations` 中标记，也要求 `confirm: true`；agent 必须先取得用户明确确认，不能自行确认。

> **安全提示**：Linux 下面板绑 `0.0.0.0` 且无鉴权，开启 MCP 后局域网内任何设备都能调用这些工具（包括切节点/切模式），请自行评估网络环境。Windows 版只听 `127.0.0.1`，无此问题。
