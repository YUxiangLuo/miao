// 规则工作台的实时预览：人话描述 + 落盘 JSON 预览。
// JSON 形状与后端保存到 config.yaml 的 custom_rules 条目一致
// （handlers/rules.rs build_rule_json：仅 port 转数字；reject 无 outbound）。

const FIELD_SENTENCE: Record<string, (value: string) => string> = {
  domain_suffix: (v) => `域名以 ${v} 结尾的站点`,
  domain: (v) => `域名为 ${v} 的站点`,
  domain_keyword: (v) => `域名包含「${v}」的站点`,
  ip_cidr: (v) => `目标 IP 在 ${v} 网段内`,
  source_ip_cidr: (v) => `来自 ${v} 网段的设备`,
  port: (v) => `目标端口为 ${v}`,
  port_range: (v) => `目标端口在 ${v} 区间`,
  protocol: (v) => `嗅探到 ${v} 协议`,
  process_name: (v) => `来自进程 ${v}`,
  process_path: (v) => `来自路径 ${v} 的进程`,
}

const TARGET_SENTENCE: Record<string, string> = {
  proxy: '走代理',
  direct: '直连',
  reject: '拦截',
}

// 人话预览：「凡是 来自进程 curl 的流量 → 直连」；值为空时返回 null
export function rulePlainPreview(field: string, value: string, target: string): string | null {
  const trimmed = (value || '').trim()
  if (!trimmed) return null
  const describe = FIELD_SENTENCE[field]
  const targetText = TARGET_SENTENCE[target] || `走节点「${target}」`
  if (!describe) return null
  return `凡是 ${describe(trimmed)} 的连接 → ${targetText}`
}

// 落盘 JSON 预览：与 config.yaml 的 custom_rules 条目同构
// （与后端 handlers/rules.rs build_rule_json 一致：仅 port 转数字）
export function ruleJsonPreview(field: string, value: string, target: string): string | null {
  const trimmed = (value || '').trim()
  if (!trimmed) return null
  const jsonValue = field === 'port' && /^\d+$/.test(trimmed) ? Number(trimmed) : trimmed
  const rule =
    target === 'reject'
      ? { [field]: jsonValue, action: 'reject' }
      : { [field]: jsonValue, action: 'route', outbound: target }
  return JSON.stringify(rule)
}
