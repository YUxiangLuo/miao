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

route_mode: global         # 可选：启动默认路由模式（rule 规则分流 / global 全局代理）
```

**数据按三层落盘**：

| 层 | 文件 | 内容 | 位置 |
| --- | --- | --- | --- |
| 稳定层 | `config.yaml` | 订阅/节点/规则等低频配置 | Linux/OpenWrt：`/etc/miao`；Windows：应用数据目录 |
| 易变层 | `volatile.yaml` | 节点选择策略、路由模式 | Unix：`/tmp/miao-sing-box`（tmpfs，系统重启后回到 config.yaml 的启动默认值）；Windows：应用数据目录（持久） |
| 状态层 | `config.json` / `.cache` / 快照 | 运行时配置与缓存 | sing-box 运行目录，可删 |

OpenWrt 的易变层写 tmpfs：切节点/切模式这类高频操作零闪存磨损。面板/进程重启（如自升级）两层都保留，选择与模式不丢失。

## MCP：让 AI agent 操作代理

配置里加一行 `mcp: true`（默认关闭），端点是 `POST http://<面板地址>/mcp`（MCP 2026-07-28，无状态 JSON-RPC，无握手无会话）。面板右下角的浮动控件可以一键开关并复制地址。

内置工具：`get_status`（服务状态）、`list_nodes`（平铺节点池）、`switch_node`（切节点，持久化）、`set_node_select`（手动⇄地区最快）、`test_delay`（测速）、`set_route_mode`（分流⇄全局）、`refresh_subscriptions`（刷新订阅）、`list_rules`、`list_connections`。

连接时服务端通过 `instructions` 告知调用者：「你的流量很可能正经过本代理，破坏性操作会自断其网」——agent 在执行热重启类操作前会先找你确认。

> **安全提示**：Linux 下面板绑 `0.0.0.0` 且无鉴权，开启 MCP 后局域网内任何设备都能调用这些工具（包括切节点/切模式），请自行评估网络环境。Windows 版只听 `127.0.0.1`，无此问题。
