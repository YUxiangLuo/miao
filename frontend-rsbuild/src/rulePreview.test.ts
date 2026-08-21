import { describe, expect, it } from '@rstest/core'
import { ruleJsonPreview, rulePlainPreview } from './rulePreview'

describe('rulePlainPreview', () => {
  it('describes process rules in plain language', () => {
    expect(rulePlainPreview('process_name', 'curl', 'direct')).toBe('凡是 来自进程 curl 的连接 → 直连')
  })

  it('describes domain and node targets', () => {
    expect(rulePlainPreview('domain_suffix', 'example.com', '香港节点')).toBe(
      '凡是 域名以 example.com 结尾的站点 的连接 → 走节点「香港节点」',
    )
    expect(rulePlainPreview('domain_keyword', 'google', 'proxy')).toBe(
      '凡是 域名包含「google」的站点 的连接 → 走代理',
    )
  })

  it('describes ports and reject', () => {
    expect(rulePlainPreview('port', '443', 'reject')).toBe('凡是 目标端口为 443 的连接 → 拦截')
    expect(rulePlainPreview('port_range', '1000:2000', 'proxy')).toBe(
      '凡是 目标端口在 1000:2000 区间 的连接 → 走代理',
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
