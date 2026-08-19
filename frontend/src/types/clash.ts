// Clash API 类型——后端只是 127.0.0.1:6262 的反代，无 Rust 侧 schema；
// 字段以 sing-box Clash API 实际返回 + 前端实际消费面（hooks/useClash.ts、
// components/Connections*.tsx、siteIcons.ts）为准，未消费字段保持宽松。

/** 分组类型：useClash.isClashProxyGroup 依赖这两个字面量 */
export type ClashGroupType = 'Selector' | 'URLTest'

export interface ClashProxy {
  /** 节点协议（Hysteria2/AnyTLS/VLESS…）或分组类型（Selector/URLTest） */
  type: string
  /** 分组：当前选中项 */
  now?: string
  /** 分组：全部候选 */
  all?: string[]
  history?: Array<{ time: string; delay: number }>
  udp?: boolean
}

export type ClashProxies = Record<string, ClashProxy>

/** GET /proxies 响应 */
export interface ClashProxiesPayload {
  proxies: ClashProxies
}

/** WS /traffic 消息 */
export interface ClashTraffic {
  up: number
  down: number
}

/** GET /proxies/{name}/delay 响应 */
export interface ClashDelay {
  delay: number
}

/** 连接元数据：sing-box 与 mihomo 字段命名不同，siteIcons.connectionDomain 按序兜底 */
export interface ClashConnectionMetadata {
  host?: string
  sniffHost?: string
  remoteDestination?: string
  destinationIP?: string
  destination?: string
  destinationPort?: string
  network?: string
  type?: string
  sourceIP?: string
  sourcePort?: string
  process?: string
  /** sing-box 进程搜索：完整路径 + 用户名，如 "/usr/lib/firefox/firefox (alice)" */
  processPath?: string
}

export interface ClashConnection {
  id: string
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
  metadata: ClashConnectionMetadata
}

/** useConnections 轮询时按前后差值/间隔补充的速率字段 */
export interface EnrichedConnection extends ClashConnection {
  uploadSpeed: number
  downloadSpeed: number
}

/** GET /connections 响应（未消费字段宽松兜底） */
export interface ClashConnectionsPayload {
  uploadTotal: number
  downloadTotal: number
  connections: ClashConnection[]
  [key: string]: unknown
}

/** useConnections 内部状态：enriched 后的连接列表 */
export interface ConnectionsInfo extends Omit<ClashConnectionsPayload, 'connections'> {
  connections: EnrichedConnection[]
}

/** connectionFilters.groupConnections 按主域名聚合后的展示组 */
export interface ConnectionGroup {
  id: string
  domain: string
  connections: EnrichedConnection[]
  downloadSpeed: number
  uploadSpeed: number
  download: number
  upload: number
  count: number
  rule: string
  ruleLabel: string
  /** 组内除主流规则外的其他规则数 */
  extraRules: number
  outbound: string
}

