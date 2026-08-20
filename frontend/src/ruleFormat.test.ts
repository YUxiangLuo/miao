import { describe, expect, it } from 'vitest'
import {
  describeRule,
  PROTOCOL_OPTIONS,
  RULE_FIELD_OPTIONS,
  RULE_TARGET_OPTIONS,
  ruleFieldLabel,
  ruleFieldOptions,
  ruleFieldTone,
  ruleTargetLabel,
} from './ruleFormat'

describe('ruleFormat', () => {
  it('covers the backend whitelist fields and targets', () => {
    expect(RULE_FIELD_OPTIONS.map((option) => option.value)).toEqual([
      'domain_suffix',
      'domain',
      'domain_keyword',
      'ip_cidr',
      'source_ip_cidr',
      'port',
      'port_range',
      'protocol',
      'process_name',
      'process_path',
    ])
    expect(RULE_TARGET_OPTIONS.map((option) => option.value)).toEqual(['proxy', 'direct', 'reject'])
  })

  it('covers the sniff protocols supported by the backend', () => {
    expect(PROTOCOL_OPTIONS.map((option) => option.value)).toEqual([
      'http',
      'tls',
      'quic',
      'stun',
      'dns',
      'bittorrent',
      'dtls',
      'ssh',
      'rdp',
      'ntp',
    ])
  })

  it('labels known fields and falls back to the raw field name', () => {
    expect(ruleFieldLabel('process_name')).toBe('进程名')
    expect(ruleFieldLabel('geosite')).toBe('geosite')
    expect(ruleTargetLabel('reject')).toBe('拦截')
    expect(ruleTargetLabel('proxy')).toBe('代理')
    expect(ruleTargetLabel(undefined)).toBe('未知')
  })

  it('assigns a badge tone to every whitelisted field', () => {
    const tones = RULE_FIELD_OPTIONS.map((option) => ruleFieldTone(option.value))
    // 每个白名单字段都有具名色调，分类 chip 不用红/绿（绿专属直连），同族字段同色
    tones.forEach((tone) => expect(['info', 'warning', 'accent', 'neutral']).toContain(tone))
    expect(ruleFieldTone('domain')).toBe(ruleFieldTone('domain_suffix'))
    expect(ruleFieldTone('process_name')).toBe(ruleFieldTone('process_path'))
    expect(ruleFieldTone('unknown-field')).toBe('neutral')
  })

  it('uses windows process placeholders without changing field values', () => {
    const options = ruleFieldOptions('windows')
    expect(options.find((option) => option.value === 'process_name')?.placeholder).toBe(
      'qbittorrent.exe',
    )
    expect(options.find((option) => option.value === 'process_path')?.placeholder).toContain(
      'Program Files',
    )
    expect(ruleFieldOptions('linux').find((option) => option.value === 'process_name')?.placeholder).toBe(
      'curl',
    )
  })

  it('describes structured rules from the API', () => {
    expect(describeRule({ index: 0, field: 'port', value: '25', target: 'reject', skipped: false, raw: '' }))
      .toEqual({
        structured: true,
        field: 'port',
        fieldLabel: '目标端口',
        value: '25',
        target: 'reject',
      })
  })

  it('keeps a missing target as null instead of guessing proxy', () => {
    const display = describeRule({ index: 0, field: 'process_name', value: 'curl', skipped: false, raw: '{}' })
    if (!display.structured) throw new Error('expected structured description')
    expect(display.target).toBeNull()
  })

  it('falls back to raw for hand-written rules', () => {
    const raw = '{"rule_set":["custom"],"action":"route","outbound":"direct"}'
    expect(describeRule({ index: 1, skipped: false, raw })).toEqual({ structured: false, raw })
  })
})
