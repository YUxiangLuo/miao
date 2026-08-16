import { describe, expect, it } from 'vitest'
import {
  describeRule,
  PROTOCOL_OPTIONS,
  RULE_FIELD_OPTIONS,
  RULE_TARGET_OPTIONS,
  ruleFieldLabel,
  ruleFieldOptions,
  ruleTargetLabel,
} from './ruleFormat.js'

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
    expect(describeRule({ index: 0, field: 'port', value: '25', target: 'reject', raw: '' }))
      .toEqual({
        structured: true,
        field: 'port',
        fieldLabel: '目标端口',
        value: '25',
        target: 'reject',
      })
  })

  it('keeps a missing target as null instead of guessing proxy', () => {
    const display = describeRule({ index: 0, field: 'process_name', value: 'curl', raw: '{}' })
    expect(display.structured).toBe(true)
    expect(display.target).toBeNull()
  })

  it('falls back to raw for hand-written rules', () => {
    const raw = '{"rule_set":["custom"],"action":"route","outbound":"direct"}'
    expect(describeRule({ index: 1, raw })).toEqual({ structured: false, raw })
  })
})
