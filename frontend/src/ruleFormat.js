// 自定义规则的字段/目标选项与展示格式化
// 与后端 src/validation.rs 的白名单保持一致

export const RULE_FIELD_OPTIONS = [
  { value: 'domain_suffix', label: '域名后缀', placeholder: 'example.com' },
  { value: 'domain', label: '精确域名', placeholder: 'www.example.com' },
  { value: 'domain_keyword', label: '域名关键词', placeholder: 'google' },
  { value: 'ip_cidr', label: 'IP/CIDR', placeholder: '192.168.0.0/16' },
  { value: 'port', label: '目标端口', placeholder: '443' },
  { value: 'process_name', label: '进程名', placeholder: 'curl' },
  { value: 'process_path', label: '进程路径', placeholder: '/usr/bin/curl' },
]

export const RULE_TARGET_OPTIONS = [
  { value: 'proxy', label: '代理' },
  { value: 'direct', label: '直连' },
  { value: 'reject', label: '拦截' },
]

export function ruleFieldLabel(field) {
  return RULE_FIELD_OPTIONS.find((option) => option.value === field)?.label || field
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
