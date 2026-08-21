// Miao 后端 API 类型——与 crates/miao-core/src/models/*.rs 的 serde 结构体一一对应。
// 约定：serde skip_serializing_if = "Option::is_none" → TS 可选字段（?:）；
// 裸 Option<T> 序列化为 null → TS `| null`。后端字段为 snake_case，保持原名不映射。

export type RouteMode = 'rule' | 'global'

export type NodeSelect =
  | 'manual'
  | 'fastest_hk'
  | 'fastest_jp'
  | 'fastest_tw'
  | 'fastest_sg'
  | 'fastest_us'

export type RuntimePhase =
  | 'initializing'
  | 'extracting'
  | 'validating'
  | 'fetching_subscriptions'
  | 'starting'
  | 'ready'
  | 'refreshing_subscriptions'
  | 'applying_config'
  | 'reloading'
  | 'stopping'
  | 'stopped'
  | 'failed'

/** validation.rs VALID_NODE_TYPES */
export type NodeType = 'hysteria2' | 'anytls' | 'ss' | 'vmess' | 'vless' | 'trojan' | 'tuic'

/** models/api.rs ApiResponse<T>：所有 /api 端点的统一信封 */
export interface ApiResponse<T = unknown> {
  success: boolean
  message: string
  data?: T
}

/** models/api.rs StatusData（GET /api/status） */
export interface StatusData {
  running: boolean
  /** Additive fields; fallbacks in useStatus keep older backends usable. */
  ready?: boolean
  phase?: RuntimePhase
  initializing: boolean
  route_mode: RouteMode
  node_select: NodeSelect
  pid?: number
  uptime_secs?: number
  warning?: string
  /** Additive structured form; older backends may omit it. */
  warnings?: RuntimeWarning[]
  vps_supported: boolean
  platform: string
  mcp: boolean
}

export interface RuntimeWarning {
  code: string
  message: string
  severity: 'warning' | 'error' | 'info'
}

/** models/api.rs ConnectivityResult（POST /api/connectivity） */
export interface ConnectivityResult {
  name: string
  url: string
  latency_ms: number | null
  success: boolean
}

/** models/node.rs NodeInfo（GET /api/nodes） */
export interface NodeInfo {
  tag: string
  server: string
  server_port: number
  node_type: string
  sni?: string
}

/** models/api.rs RuleInfo（GET /api/rules） */
export interface RuleInfo {
  index: number
  field?: string
  value?: string
  target?: string
  /** 出口节点不存在，生成配置时被跳过（未生效） */
  skipped: boolean
  raw: string
}

/** models/api.rs SubStatus（GET /api/subs 的 data 元素） */
export interface SubStatus {
  url: string
  success: boolean
  node_count: number
  state?: 'pending' | 'refreshing' | 'ready' | 'failed'
  error?: string
}

/** models/version.rs VersionInfo（GET /api/version） */
export interface VersionInfo {
  current: string
  latest: string | null
  has_update: boolean
  download_url: string | null
  upgrade_supported: boolean
}

// ---- 请求体 ----

/** models/api.rs SubRequest（POST /api/subs） */
export interface SubRequest {
  url: string
}

/** models/api.rs RuleRequest（POST /api/rules） */
export interface RuleRequest {
  field: string
  value: string
  target: string
}

/** models/api.rs DeleteRuleRequest（DELETE /api/rules） */
export interface DeleteRuleRequest {
  index: number
  raw: string
}

/** models/api.rs McpRequest（POST /api/mcp） */
export interface McpRequest {
  enabled: boolean
}

/** models/api.rs RouteModeRequest（POST /api/route-mode） */
export interface RouteModeRequest {
  route_mode: RouteMode
}

/** models/api.rs NodeSelectRequest（POST /api/node-select） */
export interface NodeSelectRequest {
  node_select: NodeSelect
}

/** models/proxy.rs LastProxy（POST /api/last-proxy） */
export interface LastProxy {
  group: string
  name: string
}

/** models/api.rs VpsDeployRequest（POST /api/vps/deploy，仅非 Windows 构建） */
export interface VpsDeployRequest {
  ip: string
  password: string
}

/** models/api.rs VpsDeployResponse */
export interface VpsDeployResponse {
  tag: string
}

/**
 * models/node.rs NodeRequest（POST /api/nodes）。
 * 面板表单（utils.ts EMPTY_NODE_FORM）的超集：serde default 字段在 TS 侧仍必填，
 * 由表单默认值兜底；Option<String> 等可空字段在提交前由 nodeForm.ts 修剪。
 */
export interface NodeRequest {
  node_type?: NodeType
  tag: string
  server: string
  server_port: number
  /** serde default：仅密码类协议提交，其余协议省略（nodeForm 按能力裁剪） */
  password?: string
  uuid?: string
  alter_id?: number
  sni?: string
  cipher?: string
  /** serde default：SS 省略，其余协议按表单提交 */
  skip_cert_verify?: boolean
  tls_enabled?: boolean
  transport_type?: string
  transport_path?: string
  transport_host?: string
  grpc_service_name?: string
  client_fingerprint?: string
  reality_public_key?: string
  reality_short_id?: string
  flow?: string
  packet_encoding?: string
  alpn?: string[]
  tuic_congestion_control?: string
  tuic_udp_relay_mode?: string
  tuic_zero_rtt?: boolean
  obfs_type?: string
  obfs_password?: string
}

export interface BatchNodeResult {
  added: Array<{ index: number; tag: string }>
  failed: Array<{ index: number; tag: string; message: string }>
}
