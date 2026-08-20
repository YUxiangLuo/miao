// Utility functions and constants

import type { NodeType } from './types/api'

export const API_HEADERS = { 'Content-Type': 'application/json' } as const
export const POLL_INTERVAL = 3000
// /api/status 连续失败多少次判定为后端不可达：触发断线提示，并阻止把宕机误判成引导页
export const STATUS_FAILURE_THRESHOLD = 3

/** 手动节点表单状态：提交前经 nodeForm.buildNodeRequest 修剪为 NodeRequest */
export interface NodeForm {
  tag: string
  server: string
  server_port: number
  password: string
  uuid: string
  alter_id: number
  sni: string
  cipher: string
  vmess_cipher: string
  skip_cert_verify: boolean
  tls_enabled: boolean
  transport_type: string
  transport_path: string
  transport_host: string
  grpc_service_name: string
  client_fingerprint: string
  reality_public_key: string
  reality_short_id: string
  flow: string
  packet_encoding: string
  tuic_congestion_control: string
  tuic_udp_relay_mode: string
  tuic_zero_rtt: boolean
  obfs_type: string
  obfs_password: string
}

export const EMPTY_NODE_FORM: NodeForm = {
  tag: '',
  server: '',
  server_port: 443,
  password: '',
  uuid: '',
  alter_id: 0,
  sni: '',
  cipher: '2022-blake3-aes-128-gcm',
  vmess_cipher: 'auto',
  skip_cert_verify: false,
  tls_enabled: true,
  transport_type: 'tcp',
  transport_path: '',
  transport_host: '',
  grpc_service_name: '',
  client_fingerprint: '',
  reality_public_key: '',
  reality_short_id: '',
  flow: '',
  packet_encoding: '',
  tuic_congestion_control: 'cubic',
  tuic_udp_relay_mode: 'native',
  tuic_zero_rtt: false,
  obfs_type: '',
  obfs_password: '',
}

export interface NodeTypeOption {
  value: NodeType
  label: string
}

export const NODE_TYPE_OPTIONS: NodeTypeOption[] = [
  { value: 'hysteria2', label: 'Hysteria2' },
  { value: 'anytls', label: 'AnyTLS' },
  { value: 'ss', label: 'Shadowsocks' },
  { value: 'vmess', label: 'VMess' },
  { value: 'vless', label: 'VLESS' },
  { value: 'trojan', label: 'Trojan' },
  { value: 'tuic', label: 'TUIC' },
]

export interface NodeCapabilities {
  password: boolean
  uuid: boolean
  transport: boolean
  tlsToggle: boolean
}

const NODE_CAPABILITIES: Record<NodeType, NodeCapabilities> = {
  hysteria2: { password: true, uuid: false, transport: false, tlsToggle: false },
  anytls:    { password: true, uuid: false, transport: false, tlsToggle: false },
  ss:        { password: true, uuid: false, transport: false, tlsToggle: false },
  vmess:     { password: false, uuid: true, transport: true, tlsToggle: true },
  vless:     { password: false, uuid: true, transport: true, tlsToggle: true },
  trojan:    { password: true, uuid: false, transport: true, tlsToggle: false },
  tuic:      { password: true, uuid: true, transport: false, tlsToggle: false },
}

export function nodeCapabilities(type: string): NodeCapabilities {
  return (NODE_CAPABILITIES as Record<string, NodeCapabilities>)[type] || { password: false, uuid: false, transport: false, tlsToggle: false }
}

export interface SelectOption {
  value: string
  label: string
}

export const HYSTERIA2_OBFS_OPTIONS: SelectOption[] = [
  { value: '', label: '禁用混淆' },
  { value: 'salamander', label: 'Salamander' },
  { value: 'gecko', label: 'Gecko' },
]

export const CIPHER_OPTIONS = [
  '2022-blake3-aes-128-gcm',
  '2022-blake3-aes-256-gcm',
  '2022-blake3-chacha20-poly1305',
  'aes-128-gcm',
  'aes-256-gcm',
  'chacha20-ietf-poly1305',
]

export const VMESS_CIPHER_OPTIONS = ['auto', 'none', 'zero', 'aes-128-gcm', 'chacha20-poly1305']

export const TRANSPORT_OPTIONS: SelectOption[] = [
  { value: 'tcp', label: 'TCP' },
  { value: 'ws', label: 'WebSocket' },
  { value: 'http', label: 'HTTP' },
  { value: 'h2', label: 'HTTP/2' },
  { value: 'grpc', label: 'gRPC' },
]

export const CLIENT_FINGERPRINT_OPTIONS: SelectOption[] = [
  { value: '', label: '默认' },
  { value: 'chrome', label: 'Chrome' },
  { value: 'firefox', label: 'Firefox' },
  { value: 'edge', label: 'Edge' },
  { value: 'safari', label: 'Safari' },
  { value: 'ios', label: 'iOS' },
  { value: 'android', label: 'Android' },
  { value: 'random', label: 'Random' },
  { value: 'randomized', label: 'Randomized' },
]

export const PACKET_ENCODING_OPTIONS: SelectOption[] = [
  { value: '', label: '默认' },
  { value: 'xudp', label: 'xudp' },
  { value: 'packetaddr', label: 'packetaddr' },
]

export const TUIC_CONGESTION_OPTIONS: SelectOption[] = [
  { value: 'cubic', label: 'cubic' },
  { value: 'new_reno', label: 'new_reno' },
  { value: 'bbr', label: 'bbr' },
]

export const TUIC_UDP_RELAY_OPTIONS: SelectOption[] = [
  { value: 'native', label: 'native' },
  { value: 'quic', label: 'quic' },
]

export function nodeTypeDefaults(type: string): Partial<NodeForm> {
  return {
    tls_enabled: !['ss', 'vmess'].includes(type),
    cipher: '2022-blake3-aes-128-gcm',
    vmess_cipher: 'auto',
    sni: '',
    skip_cert_verify: false,
    transport_type: 'tcp',
    transport_path: '',
    transport_host: '',
    grpc_service_name: '',
    client_fingerprint: '',
    reality_public_key: '',
    reality_short_id: '',
    flow: '',
    packet_encoding: '',
    tuic_congestion_control: 'cubic',
    tuic_udp_relay_mode: 'native',
    tuic_zero_rtt: false,
    obfs_type: '',
    obfs_password: '',
  }
}

export function classNames(...items: Array<string | false | null | undefined>): string {
  return items.filter(Boolean).join(' ')
}

export function formatUptime(seconds: number | null | undefined): string {
  if (!seconds) return '--'
  const hrs = Math.floor(seconds / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  const secs = Math.floor(seconds % 60)
  if (hrs > 0) return `${hrs}h ${mins}m`
  if (mins > 0) return `${mins}m ${secs}s`
  return `${secs}s`
}

export function formatSpeed(bytes: number | null | undefined): string {
  if (!bytes) return '0 B/s'
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** index
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[index]}`
}

export function formatBytes(bytes: number | null | undefined): string {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** index
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[index]}`
}

export type DelayTone = 'neutral' | 'timeout' | 'fast' | 'medium' | 'slow'

export function getDelayTone(delay: number | null | undefined): DelayTone {
  if (delay === undefined || delay === null) return 'neutral'
  if (delay < 0) return 'timeout'
  if (delay < 80) return 'fast'
  if (delay < 180) return 'medium'
  return 'slow'
}

export function formatDelay(delay: number | null | undefined): string {
  if (delay === undefined || delay === null) return '--'
  if (delay < 0) return '超时'
  return `${delay} ms`
}

// 协议 chip 色调（.badge 变体之一）：常见协议显式映射，
// 未知协议按名称哈希到色调之一——同一协议永远同色。
// 分类 chip 不用红/绿：danger 保留给错误/拦截等警示语义，success 专属直连
export type BadgeTone = 'accent' | 'info' | 'success' | 'warning' | 'neutral' | 'danger'

const PROTOCOL_TONES: Record<string, BadgeTone> = {
  hysteria2: 'accent',
  hysteria: 'accent',
  anytls: 'info',
  vless: 'neutral',
  vmess: 'warning',
  trojan: 'neutral',
}
const PROTOCOL_TONE_FALLBACKS: BadgeTone[] = ['info', 'warning', 'accent']

export function protocolTone(protocol: string | null | undefined): BadgeTone {
  const key = (protocol || '').toLowerCase()
  if (PROTOCOL_TONES[key]) return PROTOCOL_TONES[key]
  let hash = 0
  for (const ch of key) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0
  return PROTOCOL_TONE_FALLBACKS[hash % PROTOCOL_TONE_FALLBACKS.length]
}

export function protocolLabel(type: string): string {
  const map: Record<string, string> = {
    hysteria2: 'hysteria2',
    anytls: 'anytls',
    vmess: 'vmess',
    vless: 'vless',
    trojan: 'trojan',
    tuic: 'tuic',
    shadowsocks: 'shadowsocks',
    ss: 'shadowsocks',
  }
  return map[type] || type || 'unknown'
}

export function maskSubscription(url: string): string {
  try {
    const parsed = new URL(url)
    const compactPath = parsed.pathname.length > 12 ? `...${parsed.pathname.slice(-8)}` : parsed.pathname
    return `${parsed.hostname}${compactPath || ''}`
  } catch {
    return url.length > 28 ? `${url.slice(0, 24)}...` : url
  }
}

// Validation functions：返回错误文案，null 表示通过
export function validateSubscriptionUrl(url: string): string | null {
  if (!url || !url.trim()) return '订阅链接不能为空'
  if (url.length > 4096) return '订阅链接过长'
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      return '订阅链接必须使用 HTTP 或 HTTPS 协议'
    }
    if (!parsed.hostname) return '订阅链接缺少有效的主机名'
  } catch {
    return '无效的订阅链接格式'
  }
  return null
}

export function validateNodeTag(tag: string): string | null {
  if (!tag || !tag.trim()) return '节点名称不能为空'
  if (Array.from(tag).length > 64) return '节点名称不能超过 64 个字符'
  if (!/^[\p{L}\p{N}\-_\s]+$/u.test(tag)) return '节点名称只能包含字母、数字、空格、下划线和连字符'
  return null
}

export function validateServer(server: string): string | null {
  if (!server || !server.trim()) return '服务器地址不能为空'
  if (server.length > 253) return '服务器地址过长'

  // 检查是否为有效的 IP 地址
  const ipv4Regex = /^(\d{1,3}\.){3}\d{1,3}$/
  const ipv6Regex = /^([0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}$/
  if (ipv4Regex.test(server) || ipv6Regex.test(server)) {
    return null
  }

  // 处理 FQDN 末尾的点号
  const trimmed = server.replace(/\.$/, '')

  // 域名验证
  if (!trimmed.includes('.')) {
    return '域名必须包含点号'
  }

  const parts = trimmed.split('.')
  for (const part of parts) {
    if (!part) return '域名部分不能为空'
    if (part.length > 63) return '域名的每个部分不能超过 63 个字符'
    if (part.startsWith('-') || part.endsWith('-')) return '域名部分不能以连字符开头或结尾'
    if (!/^[a-zA-Z0-9-]+$/.test(part)) return '域名部分只能包含字母、数字和连字符'
  }

  return null
}

export function validatePort(port: number | string): string | null {
  const num = Number(port)
  if (!Number.isInteger(num) || num <= 0) return '端口号必须为正整数'
  if (num > 65535) return '端口号超出范围'
  return null
}

export function validatePassword(password: string): string | null {
  if (!password || !password.trim()) return '密码不能为空'
  if (password.length < 8) return '密码太短（至少 8 个字符）'
  if (password.length > 256) return '密码过长（最多 256 个字符）'
  return null
}

export function validateUuid(uuid: string): string | null {
  if (!uuid || !uuid.trim()) return 'UUID 不能为空'
  if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(uuid.trim())) {
    return 'UUID 格式无效'
  }
  return null
}

export function validateTransport(type: string, path?: string, host?: string, serviceName?: string): string | null {
  if (!['tcp', 'ws', 'http', 'h2', 'grpc'].includes(type)) return '不支持的传输层类型'
  if (['ws', 'http', 'h2'].includes(type)) {
    if (path?.trim() && !path.trim().startsWith('/')) return '传输层路径必须以 / 开头'
    if (host?.trim() && /\s/.test(host.trim())) return 'Host 不能包含空白字符'
  }
  if (type === 'grpc' && serviceName && serviceName.length > 256) return 'gRPC service name 过长'
  return null
}

export interface TransportPayload {
  transport_type: string
  transport_path?: string
  transport_host?: string
  grpc_service_name?: string
}

export function buildTransportPayload(form: Pick<NodeForm, 'transport_type' | 'transport_path' | 'transport_host' | 'grpc_service_name'>): TransportPayload {
  const payload: TransportPayload = { transport_type: form.transport_type }

  if (['ws', 'http', 'h2'].includes(form.transport_type)) {
    if (form.transport_path?.trim()) payload.transport_path = form.transport_path.trim()
    if (form.transport_host?.trim()) payload.transport_host = form.transport_host.trim()
  }

  if (form.transport_type === 'grpc' && form.grpc_service_name?.trim()) {
    payload.grpc_service_name = form.grpc_service_name.trim()
  }

  return payload
}

export function validateVlessFlow(flow: string): string | null {
  if (!flow) return null
  if (flow !== 'xtls-rprx-vision') return '不支持的 VLESS flow'
  return null
}

export function validateHysteria2Obfs(type: string, password?: string): string | null {
  if (!type) {
    if (password?.trim()) return '请先选择混淆类型'
    return null
  }
  if (!['salamander', 'gecko'].includes(type)) return '不支持的 Hysteria2 混淆类型'
  if (!password || !password.trim()) return '混淆密码不能为空'
  if (password.length > 256) return '混淆密码过长（最多 256 个字符）'
  return null
}
