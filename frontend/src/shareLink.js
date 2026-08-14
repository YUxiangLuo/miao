// 节点分享链接解析:hysteria2 / ss / vmess / vless / trojan / tuic / anytls
// 解析结果映射为 EMPTY_NODE_FORM 的字段补丁,交由 buildNodeRequest 统一校验

const SUPPORTED_SCHEMES = ['hysteria2', 'hy2', 'ss', 'vmess', 'vless', 'trojan', 'tuic', 'anytls']

function decodeBase64(input) {
  const normalized = String(input).replace(/-/g, '+').replace(/_/g, '/')
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4)
  const binary = atob(padded)
  const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0))
  return new TextDecoder('utf-8').decode(bytes)
}

function boolParam(value) {
  return value === '1' || value === 'true'
}

// 节点名称只允许字母/数字/-_/空格(与后端校验一致),清洗链接里的 emoji 等字符
export function sanitizeTag(raw) {
  const cleaned = String(raw || '')
    .replace(/[^\p{L}\p{N}\-_\s]/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 64)
  return cleaned || ''
}

function parseUrl(line) {
  let url
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

function tagFromUrl(url, fallback) {
  const fromHash = url.hash ? decodeURIComponent(url.hash.slice(1)) : ''
  return sanitizeTag(fromHash) || fallback
}

function transportPatch(params) {
  const type = params.get('type') || 'tcp'
  if (!['tcp', 'ws', 'http', 'h2', 'grpc'].includes(type)) {
    throw new Error(`不支持的传输层: ${type}`)
  }
  const patch = { transport_type: type }
  if (params.get('path')) patch.transport_path = params.get('path')
  if (params.get('host')) patch.transport_host = params.get('host')
  if (type === 'grpc' && params.get('serviceName')) patch.grpc_service_name = params.get('serviceName')
  return patch
}

function parseHysteria2(url) {
  const params = url.searchParams
  const password = decodeURIComponent(url.username + (url.password ? `:${url.password}` : ''))
  if (!password) throw new Error('缺少密码')
  const patch = {
    password,
    tls_enabled: true,
    skip_cert_verify: boolParam(params.get('insecure')),
    sni: params.get('sni') || '',
  }
  if (params.get('obfs') && params.get('obfs') !== 'none') {
    patch.obfs_type = params.get('obfs')
    patch.obfs_password = params.get('obfs-password') || ''
  }
  return { nodeType: 'hysteria2', formPatch: patch, tag: tagFromUrl(url, 'hysteria2 节点') }
}

function parseSs(line) {
  const body = line.slice('ss://'.length)
  const hashIndex = body.indexOf('#')
  const main = hashIndex >= 0 ? body.slice(0, hashIndex) : body
  const tag = sanitizeTag(hashIndex >= 0 ? decodeURIComponent(body.slice(hashIndex + 1)) : '')

  // 旧版: ss://base64(method:password@host:port)
  if (!main.includes('@')) {
    const queryIndex = main.indexOf('?')
    const encoded = queryIndex >= 0 ? main.slice(0, queryIndex) : main
    const params = new URLSearchParams(queryIndex >= 0 ? main.slice(queryIndex + 1) : '')
    let decoded
    try {
      decoded = decodeBase64(encoded)
    } catch {
      throw new Error('SS 链接 base64 解码失败')
    }
    return parseSsUserinfo(decoded, tag, params)
  }

  // SIP002: ss://base64(method:password)@host:port?...#name
  const atIndex = main.lastIndexOf('@')
  const userinfo = main.slice(0, atIndex)
  const hostportAndQuery = main.slice(atIndex + 1)
  const queryIndex = hostportAndQuery.indexOf('?')
  const hostport = queryIndex >= 0 ? hostportAndQuery.slice(0, queryIndex) : hostportAndQuery
  const params = new URLSearchParams(queryIndex >= 0 ? hostportAndQuery.slice(queryIndex + 1) : '')

  let credentials = userinfo
  // userinfo 可能是 base64(method:password),也可能是明文 method:password
  if (!userinfo.includes(':')) {
    try {
      credentials = decodeBase64(userinfo)
    } catch {
      throw new Error('SS 链接用户信息解码失败')
    }
  }
  return parseSsUserinfo(`${credentials}@${hostport}`, tag, params)
}

function parseSsUserinfo(combined, tag, params) {
  // combined: method:password@host:port(密码可能含 @,取最后一个 @)
  const atIndex = combined.lastIndexOf('@')
  if (atIndex < 0) throw new Error('SS 链接缺少 @')
  const userinfo = combined.slice(0, atIndex)
  const hostport = combined.slice(atIndex + 1)
  const colonIndex = userinfo.indexOf(':')
  if (colonIndex < 0) throw new Error('SS 链接缺少加密方式')
  const method = userinfo.slice(0, colonIndex)
  const password = decodeURIComponent(userinfo.slice(colonIndex + 1))
  const portMatch = hostport.match(/:(\d+)$/)
  if (!portMatch) throw new Error('SS 链接缺少端口')
  const server = hostport.slice(0, hostport.length - portMatch[0].length)
  if (!server) throw new Error('SS 链接缺少服务器地址')
  if (params.get('plugin')) throw new Error('暂不支持带插件的 SS 节点')

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

function parseVmess(line) {
  let json
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

  const formPatch = {
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
  return { nodeType: 'vmess', formPatch, tag: sanitizeTag(json.ps) || 'VMess 节点' }
}

function parseVless(url) {
  const params = url.searchParams
  if (!url.username) throw new Error('缺少 UUID')
  const security = params.get('security') || 'none'
  const formPatch = {
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

function parseTrojan(url) {
  const params = url.searchParams
  const password = decodeURIComponent(url.username)
  if (!password) throw new Error('缺少密码')
  const formPatch = {
    password,
    tls_enabled: true,
    sni: params.get('sni') || '',
    skip_cert_verify: boolParam(params.get('allowInsecure')),
    client_fingerprint: params.get('fp') || '',
    ...transportPatch(params),
  }
  return { nodeType: 'trojan', formPatch, tag: tagFromUrl(url, 'Trojan 节点') }
}

function parseTuic(url) {
  const params = url.searchParams
  if (!url.username || !url.password) throw new Error('TUIC 链接需要 uuid:password 格式')
  const formPatch = {
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

function parseAnytls(url) {
  const params = url.searchParams
  const password = decodeURIComponent(url.username)
  if (!password) throw new Error('缺少密码')
  const formPatch = {
    password,
    tls_enabled: true,
    sni: params.get('sni') || '',
    skip_cert_verify: boolParam(params.get('insecure')),
  }
  return { nodeType: 'anytls', formPatch, tag: tagFromUrl(url, 'AnyTLS 节点') }
}

export function parseShareLink(line) {
  const text = String(line || '').trim()
  if (!text) throw new Error('链接不能为空')
  const scheme = text.split('://')[0].toLowerCase()
  if (!SUPPORTED_SCHEMES.includes(scheme)) {
    throw new Error(`不支持的链接类型: ${scheme || '无法识别'}`)
  }

  if (scheme === 'ss') return parseSs(text)
  if (scheme === 'vmess') return parseVmess(text)

  const url = parseUrl(text)
  const parsed = (() => {
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
      default:
        throw new Error(`不支持的链接类型: ${scheme}`)
    }
  })()

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
export function parseShareLinks(text) {
  return String(text || '')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      try {
        return { line, ok: true, parsed: parseShareLink(line) }
      } catch (error) {
        return { line, ok: false, error: error.message }
      }
    })
}
