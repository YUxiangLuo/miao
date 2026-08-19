// 自定义规则的字段/目标选项与展示格式化
// 与后端 src/validation.rs 的白名单保持一致

export const RULE_FIELD_OPTIONS = [
  { value: 'domain_suffix', label: '域名后缀', placeholder: 'example.com' },
  { value: 'domain', label: '精确域名', placeholder: 'www.example.com' },
  { value: 'domain_keyword', label: '域名关键词', placeholder: 'google' },
  { value: 'ip_cidr', label: 'IP/CIDR', placeholder: '192.168.0.0/16' },
  { value: 'source_ip_cidr', label: '来源 IP/CIDR', placeholder: '192.168.1.0/24' },
  { value: 'port', label: '目标端口', placeholder: '443' },
  { value: 'port_range', label: '端口范围', placeholder: '1000:2000' },
  { value: 'protocol', label: '嗅探协议', placeholder: 'quic / tls / http…' },
  { value: 'process_name', label: '进程名', placeholder: 'curl' },
  { value: 'process_path', label: '进程路径', placeholder: '/usr/bin/curl' },
]

export function ruleFieldOptions(platform = 'linux') {
  if (platform !== 'windows') {
    return RULE_FIELD_OPTIONS
  }

  return RULE_FIELD_OPTIONS.map((option) => {
    if (option.value === 'process_name') {
      return { ...option, placeholder: 'qbittorrent.exe' }
    }
    if (option.value === 'process_path') {
      return { ...option, placeholder: 'C:\\Program Files\\qBittorrent\\qbittorrent.exe' }
    }
    return option
  })
}

export const RULE_TARGET_OPTIONS = [
  { value: 'proxy', label: '代理' },
  { value: 'direct', label: '直连' },
  { value: 'reject', label: '拦截' },
]

// 嗅探协议选项,与后端 src/validation.rs 的 VALID_PROTOCOLS 保持一致
export const PROTOCOL_OPTIONS = [
  { value: 'http', label: 'http' },
  { value: 'tls', label: 'tls' },
  { value: 'quic', label: 'quic' },
  { value: 'stun', label: 'stun' },
  { value: 'dns', label: 'dns' },
  { value: 'bittorrent', label: 'bittorrent' },
  { value: 'dtls', label: 'dtls' },
  { value: 'ssh', label: 'ssh' },
  { value: 'rdp', label: 'rdp' },
  { value: 'ntp', label: 'ntp' },
]

export function ruleFieldLabel(field) {
  return RULE_FIELD_OPTIONS.find((option) => option.value === field)?.label || field
}

// 规则字段 chip 色调（.badge 变体之一）：按字段族着色，与节点协议 chip 同体系；
// 不用红色系（danger 保留给警示语义）
const RULE_FIELD_TONES = {
  domain: 'info',
  domain_suffix: 'info',
  domain_keyword: 'info',
  ip_cidr: 'success',
  source_ip_cidr: 'success',
  port: 'warning',
  port_range: 'warning',
  protocol: 'neutral',
  process_name: 'accent',
  process_path: 'accent',
}

export function ruleFieldTone(field) {
  return RULE_FIELD_TONES[field] || 'neutral'
}

export function ruleTargetLabel(target) {
  return RULE_TARGET_OPTIONS.find((option) => option.value === target)?.label || target || '未知'
}

// 规范化后端返回的 RuleInfo;手写规则可能没有 target,调用方不得臆造
export function describeRule(info) {
  if (info?.field && info?.value) {
    return {
      structured: true,
      field: info.field,
      fieldLabel: ruleFieldLabel(info.field),
      value: info.value,
      target: info.target || null,
    }
  }
  return { structured: false, raw: info?.raw || '' }
}
