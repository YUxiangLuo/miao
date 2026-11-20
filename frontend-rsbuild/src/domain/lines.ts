import type { AccessPortStatus, NodeInfo, RuleInfo } from '../types/api'
import { isSingleSeatRule } from './terminals'

export interface DedicatedLineRow {
  node: NodeInfo
  isStaticResidential: boolean
  protocolLabel: string
  detourRelay?: string
  relayExists: boolean
  boundSeatCount: number
  boundPortCount: number
  delay?: number
  testing: boolean
}

const PROTOCOL_DISPLAY: Record<string, string> = {
  socks: '静态住宅 SOCKS5',
  hysteria2: '自建专线 Hysteria2',
  vless: '专线 VLESS',
  trojan: '专线 Trojan',
  anytls: '专线 AnyTLS',
  shadowsocks: '专线 SS',
  ss: '专线 SS',
  vmess: '专线 VMess',
  tuic: '专线 TUIC',
}

export function buildDedicatedLines(input: {
  nodes: NodeInfo[]
  rules: RuleInfo[]
  accessPorts: AccessPortStatus[]
  availableTargetNames: ReadonlyArray<string>
  delays: Record<string, number>
  testingNodes: Record<string, boolean>
}): DedicatedLineRow[] {
  const seatCountByTag = new Map<string, number>()
  for (const rule of input.rules) {
    if (isSingleSeatRule(rule) && rule.target) {
      seatCountByTag.set(rule.target, (seatCountByTag.get(rule.target) ?? 0) + 1)
    }
  }

  const portCountByTag = new Map<string, number>()
  for (const ap of input.accessPorts) {
    portCountByTag.set(ap.outbound, (portCountByTag.get(ap.outbound) ?? 0) + 1)
  }

  const knownSet = new Set<string>([
    ...input.availableTargetNames,
    ...input.nodes.map((n) => n.tag),
  ])

  return input.nodes.map((node) => {
    const key = node.node_type.toLowerCase()
    const detourRelay = node.detour?.trim() || undefined
    return {
      node,
      isStaticResidential: key === 'socks',
      protocolLabel: PROTOCOL_DISPLAY[key] || node.node_type.toUpperCase(),
      detourRelay,
      relayExists: !detourRelay || knownSet.has(detourRelay),
      boundSeatCount: seatCountByTag.get(node.tag) ?? 0,
      boundPortCount: portCountByTag.get(node.tag) ?? 0,
      delay: input.delays[node.tag],
      testing: Boolean(input.testingNodes[node.tag]),
    }
  })
}
