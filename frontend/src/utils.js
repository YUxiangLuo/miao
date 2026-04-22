// Utility functions and constants

export const API_HEADERS = { 'Content-Type': 'application/json' }
export const POLL_INTERVAL = 3000

export const CONNECTIVITY_SITES = [
  { name: 'Google', url: 'https://www.google.com' },
  { name: 'GitHub', url: 'https://github.com' },
  { name: 'YouTube', url: 'https://www.youtube.com' },
  { name: 'Bilibili', url: 'https://www.bilibili.com' },
]

export const EMPTY_NODE_FORM = {
  tag: '',
  server: '',
  server_port: 443,
  password: '',
  sni: '',
  cipher: '2022-blake3-aes-128-gcm',
  skip_cert_verify: false,
}

export const CIPHER_OPTIONS = [
  '2022-blake3-aes-128-gcm',
  '2022-blake3-aes-256-gcm',
  '2022-blake3-chacha20-poly1305',
  'aes-128-gcm',
  'aes-256-gcm',
  'chacha20-ietf-poly1305',
]

export const VMESS_SECURITY_OPTIONS = [
  'auto',
  'none',
  'zero',
  'aes-128-gcm',
  'chacha20-poly1305',
  'aes-128-ctr',
]

export const VLESS_FLOW_OPTIONS = [
  '',
  'xtls-rprx-vision',
]

export const TUIC_CONGESTION_OPTIONS = [
  'bbr',
  'cubic',
  'new_reno',
]

export function classNames(...items) {
  return items.filter(Boolean).join(' ')
}

export function formatUptime(seconds) {
  if (!seconds) return '--'
  const hrs = Math.floor(seconds / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  const secs = Math.floor(seconds % 60)
  if (hrs > 0) return `${hrs}h ${mins}m`
  if (mins > 0) return `${mins}m ${secs}s`
  return `${secs}s`
}

export function formatSpeed(bytes) {
  if (!bytes) return '0 B/s'
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** index
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[index]}`
}

export function getDelayTone(delay) {
  if (delay === undefined || delay === null) return 'neutral'
  if (delay < 0) return 'timeout'
  if (delay < 80) return 'fast'
  if (delay < 180) return 'medium'
  return 'slow'
}

export function formatDelay(delay) {
  if (delay === undefined || delay === null) return '--'
  if (delay < 0) return '超时'
  return `${delay} ms`
}

export function protocolLabel(type) {
  const map = {
    hysteria2: 'hysteria2',
    anytls: 'anytls',
    shadowsocks: 'shadowsocks',
    ss: 'shadowsocks',
    trojan: 'trojan',
    vmess: 'vmess',
    vless: 'vless',
    tuic: 'tuic',
  }
  return map[type] || type || 'unknown'
}

export function maskSubscription(url) {
  try {
    const parsed = new URL(url)
    const compactPath = parsed.pathname.length > 12 ? `...${parsed.pathname.slice(-8)}` : parsed.pathname
    return `${parsed.hostname}${compactPath || ''}`
  } catch {
    return url.length > 28 ? `${url.slice(0, 24)}...` : url
  }
}

// Validation functions
export function validateSubscriptionUrl(url) {
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

export function validateNodeTag(tag) {
  if (!tag || !tag.trim()) return '节点名称不能为空'
  if (tag.length > 64) return '节点名称不能超过 64 个字符'
  if (!/^[a-zA-Z0-9\-_\s]+$/.test(tag)) return '节点名称只能包含字母、数字、空格、下划线和连字符'
  return null
}

export function validateServer(server) {
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

export function validatePort(port) {
  const num = Number(port)
  if (!Number.isInteger(num) || num <= 0) return '端口号必须为正整数'
  if (num > 65535) return '端口号超出范围'
  return null
}

export function validatePassword(password) {
  if (!password || !password.trim()) return '密码不能为空'
  if (password.length < 8) return '密码太短（至少 8 个字符）'
  if (password.length > 256) return '密码过长（最多 256 个字符）'
  return null
}
