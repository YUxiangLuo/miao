import { describe, expect, it } from '@rstest/core'
import { ruleJsonPreview, rulePlainPreview, validateRuleValue } from './rulePreview'

describe('rulePlainPreview', () => {
  it('describes process rules in plain language', () => {
    expect(rulePlainPreview('process_name', 'curl', 'direct')).toBe('应用 curl 发起连接时，直接连接。')
  })

  it('describes domain and node targets', () => {
    expect(rulePlainPreview('domain_suffix', 'example.com', '香港节点')).toBe(
      '访问 example.com 及其子域名时，使用节点「香港节点」。',
    )
    expect(rulePlainPreview('domain_keyword', 'google', 'proxy')).toBe(
      '访问域名包含「google」的网站时，走代理。',
    )
  })

  it('describes ports and reject', () => {
    expect(rulePlainPreview('port', '443', 'reject')).toBe('连接目标端口 443 时，阻止连接。')
    expect(rulePlainPreview('port_range', '1000:2000', 'proxy')).toBe(
      '连接目标端口 1000:2000 时，走代理。',
    )
  })

  it('returns null without a value', () => {
    expect(rulePlainPreview('domain_suffix', '', 'proxy')).toBeNull()
    expect(rulePlainPreview('domain_suffix', '   ', 'proxy')).toBeNull()
  })
})

describe('ruleJsonPreview', () => {
  it('builds route rules matching the backend shape', () => {
    expect(ruleJsonPreview('process_name', 'curl', 'direct')).toBe(
      '{"process_name":"curl","action":"route","outbound":"direct"}',
    )
    expect(ruleJsonPreview('domain_suffix', 'example.com', '香港节点')).toBe(
      '{"domain_suffix":"example.com","action":"route","outbound":"香港节点"}',
    )
  })

  it('only converts port values to numbers', () => {
    expect(ruleJsonPreview('port', '443', 'reject')).toBe('{"port":443,"action":"reject"}')
    // 其他字段即使全数字也保持字符串（与后端一致）
    expect(ruleJsonPreview('domain', '123', 'proxy')).toBe(
      '{"domain":"123","action":"route","outbound":"proxy"}',
    )
  })

  it('omits outbound for reject rules', () => {
    const preview = ruleJsonPreview('domain_suffix', 'ads.example.com', 'reject')
    if (preview === null) throw new Error('expected non-null preview')
    const parsed = JSON.parse(preview)
    expect(parsed).toEqual({ domain_suffix: 'ads.example.com', action: 'reject' })
  })

  it('returns null without a value', () => {
    expect(ruleJsonPreview('domain_suffix', '', 'proxy')).toBeNull()
  })
})

describe('validateRuleValue', () => {
  it('validates ports and open/closed port ranges', () => {
    expect(validateRuleValue('port', '443')).toBeNull()
    expect(validateRuleValue('port', '0')).toMatch(/1–65535/)
    expect(validateRuleValue('port', '65536')).toMatch(/1–65535/)

    expect(validateRuleValue('port_range', '1000:2000')).toBeNull()
    expect(validateRuleValue('port_range', ':2000')).toBeNull()
    expect(validateRuleValue('port_range', '1000:')).toBeNull()
    expect(validateRuleValue('port_range', ':')).toMatch(/有效范围/)
    expect(validateRuleValue('port_range', '2000:1000')).toMatch(/有效范围/)
    expect(validateRuleValue('port_range', '0:100')).toMatch(/有效范围/)
    expect(validateRuleValue('port_range', '100:65536')).toMatch(/有效范围/)
  })

  it('validates domain characters and unicode code-point length', () => {
    expect(validateRuleValue('domain_suffix', 'example.com')).toBeNull()
    expect(validateRuleValue('domain_suffix', 'https://example.com')).toMatch(/不要包含/)
    expect(validateRuleValue('process_name', '中'.repeat(256))).toBeNull()
    expect(validateRuleValue('process_name', '中'.repeat(257))).toMatch(/256/)
  })
})
