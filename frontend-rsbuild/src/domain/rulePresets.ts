export interface RulePreset {
  id: string
  label: string
  field: string
  value: string
  recommendedTarget: 'proxy' | 'direct' | 'reject'
  hint: string
}

export const RULE_PRESETS: ReadonlyArray<RulePreset> = [
  {
    id: 'openai',
    label: 'OpenAI',
    field: 'domain_suffix',
    value: 'openai.com',
    recommendedTarget: 'proxy',
    hint: '匹配 *.openai.com',
  },
  {
    id: 'anthropic',
    label: 'Claude',
    field: 'domain_suffix',
    value: 'claude.ai',
    recommendedTarget: 'proxy',
    hint: '匹配 *.claude.ai',
  },
  {
    id: 'google',
    label: 'Google',
    field: 'domain_suffix',
    value: 'google.com',
    recommendedTarget: 'proxy',
    hint: '匹配 *.google.com',
  },
  {
    id: 'github',
    label: 'GitHub',
    field: 'domain_suffix',
    value: 'github.com',
    recommendedTarget: 'proxy',
    hint: '匹配 *.github.com',
  },
  {
    id: 'telegram',
    label: 'Telegram',
    field: 'domain_suffix',
    value: 'telegram.org',
    recommendedTarget: 'proxy',
    hint: '匹配 *.telegram.org',
  },
  {
    id: 'block-quic',
    label: '拦截 QUIC',
    field: 'protocol',
    value: 'quic',
    recommendedTarget: 'reject',
    hint: '强制客户端回退至标准 TCP/TLS',
  },
]
