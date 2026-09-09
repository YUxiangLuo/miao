# 配置参考

默认查找可执行文件旁的 `config.yaml`，没有则使用平台默认路径；`--config` 可显式指定。配置文件不存在时也能启动并在面板完成配置。目录与隔离规则见 [Profile 与路径归属](profiles.md)。

## 命令行启动

```bash
sudo ./miao --config /path/to/config.yaml
sudo ./miao --sub 'https://example.com/sub?token=xxx' JP
./miao --help
./miao --version
```

`--config=PATH`、`--sub=URL` 同样有效。`--sub` 接受 HTTP/HTTPS Clash YAML 订阅，与 `--config` 互斥；参数在提权、文件写入和内核启动前校验。

地区可选 `HK / JP / TW / SG / US`，大小写不限；省略时手动选择，无该地区候选时临时回退手动并保留地区偏好。`--sub` 使用独立临时 Profile，默认规则分流、不限倍率，不读取已有配置或偏好；面板修改也仅属于本次运行。正常退出后清理，强杀、断电或进程替换升级可能留下临时目录，但后续运行不会复用。

订阅 URL 须加引号，并留意令牌会进入 shell 历史和进程参数。Profile 隔离不支持多开，TUN 和 Clash API 仍共享，不要与已有实例同时启动。

## 配置文件

```yaml
port: 6161
subs:
  - "https://your-subscription-url"
nodes:  # sing-box outbound JSON，支持范围见内核文档
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
custom_rules:
  - '{"domain_suffix":"example.com","action":"route","outbound":"direct"}'
  - '{"process_name":"qbittorrent","action":"route","outbound":"香港节点"}'

# 以下均可省略；策略/倍率的持久偏好优先于启动默认值
mcp: false
node_select: manual  # manual / fastest_hk/jp/tw/sg/us
max_multiplier: null  # 不限，或正数（例如 2.5）
route_mode: rule  # rule / global
```

手动节点支持七种协议，见[内核能力](kernel.md#能力与裁剪)。自定义规则优先于内置分流，全局模式下仍生效；出口可填 `proxy`、`direct` 或节点 tag，引用缺失/禁用节点的规则会跳过并在面板标记。Windows 进程名需带 `.exe`。

## 状态保存与节点选择

| 文件 | 保存内容 |
| --- | --- |
| `config.yaml` | 订阅、手动节点、规则、端口、MCP 开关及启动默认值 |
| `volatile.yaml` | 当前策略、倍率、路由模式和禁用的订阅节点 |
| `.node_select` / `.max_multiplier` / `.last_proxy` | 显式选择的策略、倍率和具体手动节点 |
| `config.json`、运行缓存、订阅快照 | 派生运行状态，可重新生成 |

具体位置和平台持久化差异集中在[路径表](profiles.md#文件位置)。默认 Unix 易变层在 tmpfs；OpenWrt/非 systemd 的选择偏好也在 tmpfs，进程重启时保留，系统重启后回到 YAML 启动默认值。systemd Linux 和 Windows 会持久保存选择偏好。

地区无候选而回退 `manual` 不覆盖用户请求的策略，后续刷新继续尝试原地区。首次升级会迁移旧易变层中明确记录的倍率和 `fastest_*`，但不将无法区分来源的 `manual` 提升为持久偏好。具体手动节点由 `.last_proxy` 独立恢复。

最高倍率只影响地区最快模式的自动候选，手动选择仍保留完整节点池。倍率从当前显示名识别（如 `18x`、`6.5X`、`2.4倍`、`倍率：1.3`）；未标记按 1x，明确标记但数值无效的节点不进入受限候选。下拉选项来自完整节点池，选择“不限”可恢复全部自动候选，偏好文件用 `unlimited` 表示不限。

### 开机订阅刷新失败

有兼容缓存、订阅快照或有效手动节点时，先用本地材料启核，再后台刷新。首轮最多 20 秒，随后最多 4 次快速重试，间隔 5、10、20、40 秒（另加请求耗时）。代理已就绪但订阅仍失败时，保留运行配置并告警，改为每 30 分钟静默重试。

手动刷新失败不重置快速重试额度，成功则结束启动恢复任务；停服或修改订阅会取消旧任务。代理本身尚未就绪时仍继续恢复，不受上述次数限制。刷新活动与代理状态独立，后台拉取不会把就绪的代理改成“启动中”。

成功空列表不算网络失败：有替代节点时正常应用并清理旧订阅节点；没有可用候选且当前代理可用时保留运行态，提示检查订阅或禁用设置，不反复获取空列表。缓存可用不代表本轮拉取成功。API 语义见[状态文档](runtime-state.md#订阅刷新)。

### 禁用订阅节点

在订阅卡片点击节点数量，可逐个禁用/启用。禁用节点不进入生成配置，引用它的规则也会跳过；操作后节点池为空（含手动节点）会返回 400。

禁用集由面板维护在 `volatile.yaml`，按“订阅 URL + 节点名”匹配，同订阅内同名节点一起禁用；改名后旧条目失效。默认 Unix 易变层随系统重启清空。

```yaml
disabled_nodes:
  - sub: "https://your-subscription-url"
    name: "香港 01"
```

## MCP：让 AI agent 操作代理

设置 `mcp: true` 或使用面板右下角开关，连接 `http://<面板地址>/mcp`；关闭时返回 404。当前实现 MCP `2025-11-25` Streamable HTTP：先 `initialize`，再 `notifications/initialized`，后续请求携带 `MCP-Protocol-Version`；无 session、无 SSE，启用时 `GET /mcp` 返回 405。

用 `tools/list` 获取完整工具和参数。工具覆盖状态、流量/连接、启停、节点策略/倍率、切换/测速、订阅、手动节点、规则、MCP 开关、VPS 部署和升级；平台不支持的操作返回明确错误。主题、弹窗、PWA 和分享链接解析属于浏览器本地功能；结构化节点可通过 `add_node` / `import_nodes` 导入。

停止、删除、部署、关闭 MCP、升级等破坏性工具要求 `confirm: true`，调用者须先获得用户明确确认。订阅 URL、连接记录和 VPS 密码应保密；流量可能正经过本代理，热应用配置可能影响连接。

**面板和 MCP 无鉴权。** Linux 默认监听 `0.0.0.0`，局域网设备也可调用；Windows 仅监听 `127.0.0.1`。不要直接暴露到不可信网络。
