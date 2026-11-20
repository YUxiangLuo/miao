import type { AccessPortStatus, NodeInfo } from '../types/api'
import type { ClashConnection, EnrichedConnection } from '../types/clash'

export interface AccessPortRow extends AccessPortStatus {
  inboundTag: string
  listenEndpoint: string
  detourRelay?: string
  connectionCount: number
  uploadSpeed: number
  downloadSpeed: number
  /** AdsPower / BitBrowser / Hubstudio 标准粘贴格式：host:port 或 host:port:user:pass */
  browserPasteText: string
  socks5Uri: string
  httpUri: string
}

export function resolveGatewayHost(hostname?: string): string {
  const host = (hostname ?? (typeof window !== 'undefined' ? window.location.hostname : '')).trim()
  if (!host || host === 'localhost' || host === '127.0.0.1' || host === '::1') {
    return host || '192.168.8.1'
  }
  return host
}

/** 从 Clash 连接元数据或规则文本识别所属专属端口 ID（如 `access-port:ap-20101` -> `ap-20101`） */
export function connectionAccessPortId(conn: ClashConnection): string | null {
  const metaType = conn.metadata?.type || ''
  const tagMatch = metaType.match(/access-port:([A-Za-z0-9_-]+)/)
  if (tagMatch) return tagMatch[1]

  const ruleText = `${conn.rule || ''} ${conn.rulePayload || ''}`
  const ruleMatch = ruleText.match(/access-port:([A-Za-z0-9_-]+)/)
  if (ruleMatch) return ruleMatch[1]
  return null
}

function encodeAuthPrefix(username?: string, password?: string): string {
  if (!username || !password) return ''
  return `${encodeURIComponent(username)}:${encodeURIComponent(password)}@`
}

export function buildAccessPortRows(input: {
  accessPorts: AccessPortStatus[]
  nodes: NodeInfo[]
  connections: EnrichedConnection[]
  gatewayHost?: string
}): AccessPortRow[] {
  const host = resolveGatewayHost(input.gatewayHost)
  const nodeByTag = new Map(input.nodes.map((node) => [node.tag, node]))
  const statsById = new Map<string, { count: number; up: number; down: number }>()

  for (const conn of input.connections) {
    const id = connectionAccessPortId(conn)
    if (!id) continue
    const prev = statsById.get(id) ?? { count: 0, up: 0, down: 0 }
    prev.count += 1
    prev.up += Number(conn.uploadSpeed || 0)
    prev.down += Number(conn.downloadSpeed || 0)
    statsById.set(id, prev)
  }

  return input.accessPorts
    .map((ap) => {
      const boundNode = nodeByTag.get(ap.outbound)
      const stats = statsById.get(ap.id) ?? { count: 0, up: 0, down: 0 }
      const listenEndpoint = `${host}:${ap.port}`
      const authPrefix = encodeAuthPrefix(ap.username, ap.password)
      const browserPasteText = ap.has_auth && ap.username && ap.password
        ? `${host}:${ap.port}:${ap.username}:${ap.password}`
        : listenEndpoint

      return {
        ...ap,
        inboundTag: `access-port:${ap.id}`,
        listenEndpoint,
        detourRelay: boundNode?.detour,
        connectionCount: stats.count,
        uploadSpeed: stats.up,
        downloadSpeed: stats.down,
        browserPasteText,
        socks5Uri: `socks5://${authPrefix}${listenEndpoint}`,
        httpUri: `http://${authPrefix}${listenEndpoint}`,
      }
    })
    .sort((a, b) => a.port - b.port)
}

export function suggestNextAccessPort(existing: ReadonlyArray<{ port: number }>, panelPort = 6161): number {
  const used = new Set(existing.map((item) => item.port))
  used.add(panelPort)
  used.add(6262)
  for (let candidate = 20101; candidate <= 59999; candidate++) {
    if (!used.has(candidate)) return candidate
  }
  return 20101
}
