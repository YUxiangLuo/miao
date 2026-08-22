use serde_json::{json, Value as JsonValue};

fn read_only() -> JsonValue {
    json!({ "readOnlyHint": true, "destructiveHint": false })
}

fn mutating() -> JsonValue {
    json!({ "readOnlyHint": false, "destructiveHint": false })
}

fn destructive() -> JsonValue {
    json!({ "readOnlyHint": false, "destructiveHint": true })
}

fn empty_schema() -> JsonValue {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn confirmation_property(action: &str) -> JsonValue {
    json!({
        "type": "boolean",
        "const": true,
        "description": format!("仅在用户明确确认{action}后传 true"),
    })
}

fn node_schema() -> JsonValue {
    json!({
        "type": "object",
        "properties": {
            "node_type": {
                "type": "string",
                "enum": ["hysteria2", "anytls", "ss", "vmess", "vless", "trojan", "tuic"],
                "default": "hysteria2",
                "description": "节点协议"
            },
            "tag": { "type": "string", "description": "唯一节点名称，1-64 个字符" },
            "server": { "type": "string", "description": "服务器域名、IPv4 或 IPv6" },
            "server_port": { "type": "integer", "minimum": 1, "maximum": 65535 },
            "password": { "type": "string", "description": "hysteria2/anytls/ss/trojan/tuic 的密码" },
            "uuid": { "type": "string", "description": "vmess/vless/tuic 的 UUID" },
            "alter_id": { "type": "integer", "minimum": 0, "maximum": 65535 },
            "sni": { "type": "string" },
            "cipher": { "type": "string", "description": "Shadowsocks method 或 VMess security" },
            "skip_cert_verify": { "type": "boolean", "default": false },
            "tls_enabled": { "type": "boolean" },
            "transport_type": { "type": "string", "enum": ["tcp", "ws", "http", "h2", "grpc"] },
            "transport_path": { "type": "string" },
            "transport_host": { "type": "string" },
            "grpc_service_name": { "type": "string" },
            "alpn": { "type": "array", "items": { "type": "string" } },
            "client_fingerprint": {
                "type": "string",
                "enum": ["chrome", "firefox", "edge", "safari", "360", "qq", "ios", "android", "random", "randomized"]
            },
            "reality_public_key": { "type": "string" },
            "reality_short_id": { "type": "string" },
            "flow": { "type": "string", "enum": ["xtls-rprx-vision"] },
            "packet_encoding": { "type": "string", "enum": ["packetaddr", "xudp"] },
            "tuic_congestion_control": { "type": "string", "enum": ["cubic", "new_reno", "bbr"] },
            "tuic_udp_relay_mode": { "type": "string", "enum": ["native", "quic"] },
            "tuic_zero_rtt": { "type": "boolean", "default": false },
            "obfs_type": { "type": "string", "enum": ["salamander", "gecko"] },
            "obfs_password": { "type": "string" }
        },
        "required": ["tag", "server", "server_port"],
        "additionalProperties": false
    })
}

pub(super) fn tools_catalog() -> JsonValue {
    let node = node_schema();
    json!([
        {
            "name": "get_status",
            "description": "读取 Miao 控制面与 sing-box 数据面的状态：进程是否存在、数据面是否就绪、当前阶段、模式、当前节点、节点数、运行时长和告警。无副作用。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "get_version_info",
            "description": "读取当前版本、GitHub 最新版本、是否可更新及当前平台是否支持面板内升级。内核未就绪时为避免影响启动，不请求 GitHub。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "start_service",
            "description": "启动已配置的 sing-box 数据面。会启用 TUN 并改变本机/路由器流量路径；仅在用户明确确认后调用。不会添加订阅或节点。",
            "inputSchema": {
                "type": "object",
                "properties": { "confirm": confirmation_property("启动透明代理") },
                "required": ["confirm"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "stop_service",
            "description": "停止 sing-box 数据面但保留配置。调用者的网络可能立即中断，且后续配置修改不会自动重启，直到显式 start_service；仅在用户明确确认后调用。",
            "inputSchema": {
                "type": "object",
                "properties": { "confirm": confirmation_property("停止透明代理并可能中断网络") },
                "required": ["confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "list_subscriptions",
            "description": "列出订阅 URL、刷新状态、节点数和错误。URL 可能包含敏感 token，不要在回答中无必要地完整复述。无副作用。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "add_subscriptions",
            "description": "一次事务添加一个或多个 Clash YAML 订阅；去重并跳过已存在项。会立即联网拉取订阅、生成并校验配置，服务运行时可能热重载/重启并短暂影响连接。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array", "minItems": 1, "uniqueItems": true,
                        "items": { "type": "string", "format": "uri" },
                        "description": "http/https 订阅 URL 列表"
                    }
                },
                "required": ["urls"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "delete_subscription",
            "description": "删除一条订阅。必须先用 list_subscriptions 取得精确 URL；可能导致节点消失、规则失效或停止无剩余节点的服务。仅在用户确认后调用。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "format": "uri" },
                    "confirm": confirmation_property("删除该订阅")
                },
                "required": ["url", "confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "refresh_subscriptions",
            "description": "立即真实拉取全部订阅并重建配置，不是缓存读取。仅当生成结果变化时更新运行时；失败时优先保留现有可用配置。可能热重载/重启并短暂影响已有连接。",
            "inputSchema": empty_schema(),
            "annotations": mutating(),
        },
        {
            "name": "scan_clash_verge",
            "description": "只读扫描本机 clash-verge-rev 配置中的远程订阅，并标记哪些已加入 Miao；不会导入或修改配置。需要导入时把选中的 URL 交给 add_subscriptions。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "list_nodes",
            "description": "列出运行时平铺节点池（订阅节点 + 手动节点）的名称、协议、来源和当前选择；不暴露 selector 分组。数据面未就绪时只能返回手动节点。无副作用。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "list_manual_nodes",
            "description": "列出像面板“手动节点”卡片一样的持久节点：tag、服务器、端口、协议和 SNI。不会列出订阅节点，也不会返回密码。无副作用。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "add_node",
            "description": "添加一个结构化手动节点。字段与面板手动节点表单一致；会生成并校验配置，服务运行时可能热重载/重启。协议所需字段不同，缺失时返回具体校验错误。",
            "inputSchema": node.clone(),
            "annotations": mutating(),
        },
        {
            "name": "import_nodes",
            "description": "一次事务批量添加结构化手动节点；逐项返回 added/failed，只进行一次运行时配置更新。适合导入多个已解析的分享链接。",
            "inputSchema": {
                "type": "object",
                "properties": { "nodes": { "type": "array", "minItems": 1, "items": node } },
                "required": ["nodes"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "delete_node",
            "description": "按精确 tag 删除一个手动节点，不能删除订阅节点。可能使指定该节点的规则失效，删除最后一个可用来源时会停服；仅在用户确认后调用。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tag": { "type": "string", "description": "来自 list_manual_nodes 的精确 tag" },
                    "confirm": confirmation_property("删除该手动节点")
                },
                "required": ["tag", "confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "switch_node",
            "description": "在 manual 选择策略下切换 selector 当前节点并持久化选择。不重建配置、不重启内核；通常影响新连接，已有连接的处理由 sing-box 决定。节点名必须来自 list_nodes。",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "来自 list_nodes 的精确节点名" } },
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "set_node_select",
            "description": "设置节点选择策略：manual 手动选择；fastest_hk/jp/tw/sg/us 由 sing-box 在香港/日本/台湾/新加坡/美国节点中自动测速选择。只用本地快照重建，不刷新订阅；Unix 热重载，Windows 重启内核。地区无节点时回退 manual。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "select": {
                        "type": "string",
                        "enum": ["manual", "fastest_hk", "fastest_jp", "fastest_tw", "fastest_sg", "fastest_us"]
                    }
                },
                "required": ["select"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "test_delay",
            "description": "通过 sing-box Clash API 测一个节点或全部运行时节点到固定探测 URL 的延迟。返回毫秒数，-1 表示超时/失败；批量测试并发上限为 6。无配置副作用，但会产生探测流量。",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "可选；来自 list_nodes。省略则测试全部节点" } },
                "additionalProperties": false
            },
            "annotations": read_only(),
        },
        {
            "name": "set_route_mode",
            "description": "设置路由模式：rule 使用内置国内直连/国外代理分流；global 关闭内置地域分流，但自定义规则仍优先生效。写入易变层；Unix 热重载，Windows 重启内核，可能短暂影响连接。",
            "inputSchema": {
                "type": "object",
                "properties": { "mode": { "type": "string", "enum": ["rule", "global"] } },
                "required": ["mode"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "list_rules",
            "description": "列出自定义规则的 index、结构化 field/value/target、原始 JSON 和是否因目标节点缺失而被跳过。内置地域规则不在此列表。无副作用。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "add_rule",
            "description": "添加一条面板支持的单条件自定义规则。target 可为 proxy、direct、reject 或当前存在/保留的节点 tag；自定义规则在 rule/global 两种模式下都优先。会校验并应用运行时配置。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "field": {
                        "type": "string",
                        "enum": ["domain_suffix", "domain", "domain_keyword", "ip_cidr", "source_ip_cidr", "port", "port_range", "protocol", "process_name", "process_path"]
                    },
                    "value": { "type": "string" },
                    "target": { "type": "string", "description": "proxy/direct/reject 或节点 tag" }
                },
                "required": ["field", "value", "target"],
                "additionalProperties": false
            },
            "annotations": mutating(),
        },
        {
            "name": "delete_rule",
            "description": "删除自定义规则。为避免并发误删，index 和 raw 必须原样取自最近一次 list_rules；仅在用户确认后调用。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "minimum": 0 },
                    "raw": { "type": "string", "description": "list_rules 返回的原始 JSON" },
                    "confirm": confirmation_property("删除该自定义规则")
                },
                "required": ["index", "raw", "confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "get_traffic",
            "description": "读取 sing-box Clash 流量 WebSocket 的下一帧实时上传/下载速率，单位为字节/秒；最多等待 3 秒。对应面板顶栏实时流量，无配置副作用。",
            "inputSchema": empty_schema(),
            "annotations": read_only(),
        },
        {
            "name": "list_connections",
            "description": "读取活动连接的目标、端口、网络、出口链、命中规则及累计流量，并返回总上传/下载。连接数据涉及用户网络隐私，不要无必要地完整复述。支持分页，无副作用。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "offset": { "type": "integer", "minimum": 0, "default": 0 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 100 }
                },
                "additionalProperties": false
            },
            "annotations": read_only(),
        },
        {
            "name": "test_connectivity",
            "description": "从 Miao 后端直接对指定 http/https URL 发 HEAD 请求并返回耗时；该请求明确绕过系统代理环境变量，适合诊断直连网络。无配置副作用。",
            "inputSchema": {
                "type": "object",
                "properties": { "url": { "type": "string", "format": "uri" } },
                "required": ["url"],
                "additionalProperties": false
            },
            "annotations": read_only(),
        },
        {
            "name": "set_mcp_enabled",
            "description": "持久化 MCP 开关，不重启 sing-box。设为 false 会在本次响应后立即使 /mcp 返回 404 并断开管理能力，仅在用户明确确认后执行；已连接时设为 true 是无变化操作。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "confirm": confirmation_property("关闭 MCP 端点（enabled=false 时需要；enabled=true 时也应说明）")
                },
                "required": ["enabled", "confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "deploy_vps",
            "description": "仅支持具备 ssh/askpass 的非 Windows 平台：使用用户提供的 root 密码通过 SSH 在远端 VPS 部署或复用 Miao 管理的 Hysteria2，并把节点加入配置。会修改远端主机，最长约 5 分钟；密码不持久化，也不要在回答中复述。仅在用户明确确认后调用。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ip": { "type": "string", "description": "VPS IPv4、IPv6 或域名" },
                    "password": { "type": "string", "description": "VPS root 密码，仅用于本次 SSH" },
                    "confirm": confirmation_property("修改远端 VPS 并添加节点")
                },
                "required": ["ip", "password", "confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        },
        {
            "name": "upgrade_miao",
            "description": "仅 Linux/OpenWrt 支持：检查 GitHub Release，下载并校验 SHA256，替换当前 Miao 二进制后重启整个进程。MCP 响应后端点会短暂断开，代理连接也可能中断；仅在用户明确确认后调用。Windows 必须下载安装包。",
            "inputSchema": {
                "type": "object",
                "properties": { "confirm": confirmation_property("升级并重启 Miao") },
                "required": ["confirm"],
                "additionalProperties": false
            },
            "annotations": destructive(),
        }
    ])
}
