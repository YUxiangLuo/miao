// 节点分享链接解析:hysteria2 / ss / socks5 / vmess / vless / trojan / tuic / anytls
// 解析结果映射为 EMPTY_NODE_FORM 的字段补丁,交由 buildNodeRequest 统一校验
import type { NodeType } from './types/api'
import type { NodeForm } from './utils'

const SUPPORTED_SCHEMES = ['hysteria2', 'hy2', 'ss', 'socks5', 'socks', 'vmess', 'vless', 'trojan', 'tuic', 'anytls']

/** 单条链接的解析产物：协议 + 表单补丁 + 名称 */
export interface ParsedShareLink {
  nodeType: NodeType
  formPatch: Partial<NodeForm>
  tag: string
}

export type ShareLinkResult =
  | { line: string; ok: true; parsed: ParsedShareLink }
  | { line: string; ok: false; error: string }

function decodeBase64(input: string): string {
  const normalized = String(input).replace(/-/g, '+').replace(/_/g, '/')
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4)
  const binary = atob(padded)
  const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0))
  return new TextDecoder('utf-8').decode(bytes)
}

function boolParam(value: string | null): boolean {
  return value === '1' || value === 'true'
}

// 节点名称只允许字母/数字/-_/空格(与后端校验一致),清洗链接里的 emoji 等字符
export function sanitizeTag(raw: string): string {
  const cleaned = String(raw || '')
    .replace(/[^\p{L}\p{N}\-_\s]/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 64)
  return cleaned || ''
}

function parseUrl(line: string): URL {
  let url: URL
  try {
    url = new URL(line)
  } catch {
    throw new Error('链接格式无效')
  }
  const port = Number(url.port)
  if (!url.hostname) throw new Error('缺少服务器地址')
  if (!Number.isInteger(port) || port <= 0 || port > 65535) throw new Error('缺少有效端口')
  return url
}

function tagFromUrl(url: URL, fallback: string): string {
  const fromHash = url.hash ? decodeURIComponent(url.hash.slice(1)) : ''
  return sanitizeTag(fromHash) || fallback
}

function transportPatch(params: URLSearchParams): Partial<NodeForm> {
  const type = params.get('type') || 'tcp'
  if (!['tcp', 'ws', 'http', 'h2', 'grpc'].includes(type)) {
    throw new Error(`不支持的传输层: ${type}`)
  }
  const patch: Partial<NodeForm> = { transport_type: type }
  if (params.get('path')) patch.transport_path = params.get('path')!
  if (params.get('host')) patch.transport_host = params.get('host')!
  if (type === 'grpc' && params.get('serviceName')) patch.grpc_service_name = params.get('serviceName')!
  return patch
}

function parseHysteria2(url: URL): ParsedShareLink {
  const params = url.searchParams
  const password = decodeURIComponent(url.username + (url.password ? `:${url.password}` : ''))
  if (!password) throw new Error('缺少密码')
  const patch: Partial<NodeForm> = {
    password,
    tls_enabled: true,
    skip_cert_verify: boolParam(params.get('insecure')),
    sni: params.get('sni') || '',
  }
  const obfs = params.get('obfs')
  if (obfs && obfs !== 'none') {
    patch.obfs_type = obfs
    patch.obfs_password = params.get('obfs-password') || ''
  }
  return { nodeType: 'hysteria2', formPatch: patch, tag: tagFromUrl(url, 'hysteria2 节点') }
}

function splitFragment(body: string): { main: string; tag: string } {
  const hashIndex = body.indexOf('#')
  if (hashIndex < 0) return { main: body, tag: '' }
  return {
    main: body.slice(0, hashIndex),
    tag: sanitizeTag(decodeURIComponent(body.slice(hashIndex + 1))),
  }
}

function splitQuery(text: string): { beforeQuery: string; params: URLSearchParams } {
  const queryIndex = text.indexOf('?')
  if (queryIndex < 0) return { beforeQuery: text, params: new URLSearchParams() }
  return {
    beforeQuery: text.slice(0, queryIndex),
    params: new URLSearchParams(text.slice(queryIndex + 1)),
  }
}

interface SsWireParts {
  rawCredentials: string
  rawHostPort: string
  params: URLSearchParams
  /** Plaintext SIP002 userinfo uses percent-encoding; base64-decoded userinfo is literal. */
  passwordPercentEncoded: boolean
}

function decodeLegacySsParts(main: string): SsWireParts {
  // Legacy: ss://base64(method:password@host:port)[/?plugin=...]
  const { beforeQuery, params } = splitQuery(main)
  const encoded = beforeQuery.replace(/\/+$/, '')
  let decoded: string
  try {
    decoded = decodeBase64(encoded)
  } catch {
    throw new Error('SS 链接 base64 解码失败')
  }
  const atIndex = decoded.lastIndexOf('@')
  if (atIndex < 0) throw new Error('SS 链接缺少 @')
  return {
    rawCredentials: decoded.slice(0, atIndex),
    rawHostPort: decoded.slice(atIndex + 1),
    params,
    passwordPercentEncoded: false,
  }
}

function decodeSip002SsParts(main: string): SsWireParts {
  // SIP002: ss://<base64(method:password) | method:percent_password>@host:port[/?query]
  const atIndex = main.lastIndexOf('@')
  const userinfo = main.slice(0, atIndex)
  const { beforeQuery: rawHostPort, params } = splitQuery(main.slice(atIndex + 1))

  if (userinfo.includes(':')) {
    return { rawCredentials: userinfo, rawHostPort, params, passwordPercentEncoded: true }
  }

  try {
    return {
      rawCredentials: decodeBase64(userinfo),
      rawHostPort,
      params,
      passwordPercentEncoded: false,
    }
  } catch {
    throw new Error('SS 链接用户信息解码失败')
  }
}

function parseSs(line: string): ParsedShareLink {
  const { main, tag } = splitFragment(line.slice('ss://'.length))
  const parts = main.includes('@') ? decodeSip002SsParts(main) : decodeLegacySsParts(main)

  if (parts.params.get('plugin')) {
    throw new Error('暂不支持带插件的 SS 节点')
  }

  const colonIndex = parts.rawCredentials.indexOf(':')
  if (colonIndex < 0) throw new Error('SS 链接缺少加密方式')
  const method = parts.rawCredentials.slice(0, colonIndex)
  const rawPassword = parts.rawCredentials.slice(colonIndex + 1)
  const password = parts.passwordPercentEncoded ? decodeURIComponent(rawPassword) : rawPassword

  // 端口后允许带 / (ss://...@host:8388/#name 或 /?query)
  const cleanHostPort = parts.rawHostPort.replace(/\/+$/, '')
  const portMatch = cleanHostPort.match(/:(\d+)$/)
  if (!portMatch) throw new Error('SS 链接缺少端口')

  const server = cleanHostPort
    .slice(0, cleanHostPort.length - portMatch[0].length)
    .replace(/^\[|\]$/g, '')
  if (!server) throw new Error('SS 链接缺少服务器地址')

  return {
    nodeType: 'ss',
    formPatch: {
      server,
      server_port: Number(portMatch[1]),
      cipher: method,
      password,
      tls_enabled: false,
    },
    tag: tag || 'SS 节点',
  }
}

function parseVmess(line: string): ParsedShareLink {
  let json: Record<string, string | undefined>
  try {
    json = JSON.parse(decodeBase64(line.slice('vmess://'.length)))
  } catch {
    throw new Error('VMess 链接解析失败')
  }
  const server = json.add
  const port = Number(json.port)
  if (!server) throw new Error('VMess 链接缺少服务器地址')
  if (!Number.isInteger(port) || port <= 0) throw new Error('VMess 链接缺少有效端口')
  if (!json.id) throw new Error('VMess 链接缺少 UUID')

  const net = json.net || 'tcp'
  if (!['tcp', 'ws', 'http', 'h2', 'grpc'].includes(net)) {
    throw new Error(`不支持的 VMess 传输层: ${net}`)
  }

  const formPatch: Partial<NodeForm> = {
    server,
    server_port: port,
    uuid: json.id,
    alter_id: Number(json.aid || 0),
    vmess_cipher: json.scy || json.security || 'auto',
    tls_enabled: json.tls === 'tls',
    sni: json.sni || '',
    transport_type: net,
    transport_path: json.path || '',
    transport_host: json.host || '',
    client_fingerprint: json.fp || '',
  }
  return { nodeType: 'vmess', formPatch, tag: sanitizeTag(json.ps || '') || 'VMess 节点' }
}

function parseVless(url: URL): ParsedShareLink {
  const params = url.searchParams
  if (!url.username) throw new Error('缺少 UUID')
  const security = params.get('security') || 'none'
  const formPatch: Partial<NodeForm> = {
    uuid: decodeURIComponent(url.username),
    tls_enabled: security !== 'none',
    sni: params.get('sni') || '',
    client_fingerprint: params.get('fp') || '',
    flow: params.get('flow') || '',
    packet_encoding: params.get('packetEncoding') || '',
    ...transportPatch(params),
  }
  if (security === 'reality') {
    formPatch.reality_public_key = params.get('pbk') || ''
    formPatch.reality_short_id = params.get('sid') || ''
  }
  return { nodeType: 'vless', formPatch, tag: tagFromUrl(url, 'VLESS 节点') }
}

function parseTrojan(url: URL): ParsedShareLink {
  const params = url.searchParams
  const password = decodeURIComponent(url.username)
  if (!password) throw new Error('缺少密码')
  const formPatch: Partial<NodeForm> = {
    password,
    tls_enabled: true,
    sni: params.get('sni') || '',
    skip_cert_verify: boolParam(params.get('allowInsecure')),
    client_fingerprint: params.get('fp') || '',
    ...transportPatch(params),
  }
  return { nodeType: 'trojan', formPatch, tag: tagFromUrl(url, 'Trojan 节点') }
}

function parseTuic(url: URL): ParsedShareLink {
  const params = url.searchParams
  if (!url.username || !url.password) throw new Error('TUIC 链接需要 uuid:password 格式')
  const formPatch: Partial<NodeForm> = {
    uuid: decodeURIComponent(url.username),
    password: decodeURIComponent(url.password),
    tls_enabled: true,
    sni: params.get('sni') || '',
    skip_cert_verify: boolParam(params.get('allow_insecure')),
    tuic_congestion_control: params.get('congestion_control') || 'cubic',
    tuic_udp_relay_mode: params.get('udp_relay_mode') || 'native',
  }
  return { nodeType: 'tuic', formPatch, tag: tagFromUrl(url, 'TUIC 节点') }
}

function parseAnytls(url: URL): ParsedShareLink {
  const params = url.searchParams
  const password = decodeURIComponent(url.username)
  if (!password) throw new Error('缺少密码')
  const formPatch: Partial<NodeForm> = {
    password,
    tls_enabled: true,
    sni: params.get('sni') || '',
    skip_cert_verify: boolParam(params.get('insecure')),
  }
  return { nodeType: 'anytls', formPatch, tag: tagFromUrl(url, 'AnyTLS 节点') }
}

function parseSocks(url: URL): ParsedShareLink {
  if (boolParam(url.searchParams.get('tls')) || url.searchParams.get('security') === 'tls') {
    throw new Error('SOCKS5 节点不支持 TLS 配置')
  }
  return {
    nodeType: 'socks',
    formPatch: {
      username: decodeURIComponent(url.username),
      password: decodeURIComponent(url.password),
      tls_enabled: false,
    },
    tag: tagFromUrl(url, 'SOCKS5 节点'),
  }
}

const SCHEME_PREFIX_RE = /^[a-z][a-z0-9+.-]*:\/\//iu
const BARE_SOCKS5_RE = /^(\[[^\]]+\]|[^\s:]+):(\d+):([^:]+):(.+)$/u

function parseBareSocks5Line(text: string): ParsedShareLink {
  const match = BARE_SOCKS5_RE.exec(text)
  if (!match) {
    throw new Error('不支持的链接类型；请填写分享链接或 SOCKS5 地址:端口:用户名:密码')
  }
  const [, host, port, username, password] = match
  // Percent-encode literal credentials so `:`, `@`, `%`, `://` survive URL parsing intact.
  return parseShareLink(
    `socks5://${encodeURIComponent(username)}:${encodeURIComponent(password)}@${host}:${port}`,
  )
}

function parseUrlBasedScheme(scheme: string, url: URL): ParsedShareLink {
  switch (scheme) {
    case 'hysteria2':
    case 'hy2':
      return parseHysteria2(url)
    case 'vless':
      return parseVless(url)
    case 'trojan':
      return parseTrojan(url)
    case 'tuic':
      return parseTuic(url)
    case 'anytls':
      return parseAnytls(url)
    case 'socks':
    case 'socks5':
      return parseSocks(url)
    default:
      throw new Error(`不支持的链接类型: ${scheme}`)
  }
}

export function parseShareLink(line: string): ParsedShareLink {
  const text = String(line || '').trim()
  if (!text) throw new Error('链接不能为空')
  if (!SCHEME_PREFIX_RE.test(text)) {
    return parseBareSocks5Line(text)
  }

  const scheme = text.split('://')[0].toLowerCase()
  if (!SUPPORTED_SCHEMES.includes(scheme)) {
    throw new Error(`不支持的链接类型: ${scheme || '无法识别'}`)
  }

  if (scheme === 'ss') return parseSs(text)
  if (scheme === 'vmess') return parseVmess(text)

  const url = parseUrl(text)
  const parsed = parseUrlBasedScheme(scheme, url)

  return {
    ...parsed,
    formPatch: {
      server: url.hostname.replace(/^\[|\]$/g, ''),
      server_port: Number(url.port),
      ...parsed.formPatch,
    },
  }
}

// 逐行解析,空行跳过;每行返回 { line, ok, parsed | error }
export function parseShareLinks(text: string): ShareLinkResult[] {
  return String(text || '')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line, index): ShareLinkResult => {
      try {
        const parsed = parseShareLink(line)
        if (parsed.nodeType === 'socks' && parsed.tag === 'SOCKS5 节点') {
          parsed.tag = `SOCKS5 节点 ${index + 1}`
        }
        return { line, ok: true, parsed }
      } catch (error) {
        return { line, ok: false, error: error instanceof Error ? error.message : String(error) }
      }
    })
}

/**
 * Reconciles pasted share-link lines after `/api/nodes/import` returns.
 * `addedIndices` indexes into the subset of valid (`ok: true`) payloads sent to the API,
 * NOT into `items` directly. Invalid lines and valid lines rejected by the backend are retained.
 */
export function retainUnimportedShareLinks<T extends { line: string; ok: boolean }>(
  items: readonly T[],
  addedIndices: ReadonlySet<number>,
): T[] {
  const remaining: T[] = []
  let validPayloadIndex = 0
  for (const item of items) {
    if (!item.ok) {
      remaining.push(item)
      continue
    }
    const wasAdded = addedIndices.has(validPayloadIndex)
    validPayloadIndex += 1
    if (!wasAdded) {
      remaining.push(item)
    }
  }
  return remaining
}
