import { describe, expect, it } from 'vitest'
import {
  activeRuleIndexes,
  isCustomRuleActive,
  signatureFromClashRule,
  signatureFromCustomRule,
} from './ruleActivity.js'

const curlDirect = {
  index: 0,
  field: 'process_name',
  value: 'curl',
  target: 'direct',
  raw: '{"process_name":"curl","action":"route","outbound":"direct"}',
}

describe('signatureFromClashRule', () => {
  it('parses route and reject forms from sing-box Clash API', () => {
    expect(signatureFromClashRule('process_name=curl => route(direct)')).toEqual({
      field: 'process_name',
      value: 'curl',
      target: 'direct',
    })
    expect(signatureFromClashRule('domain_keyword=openai => route(Deguo)')).toEqual({
      field: 'domain_keyword',
      value: 'openai',
      target: 'Deguo',
    })
    expect(signatureFromClashRule('protocol=bittorrent => reject')).toEqual({
      field: 'protocol',
      value: 'bittorrent',
      target: 'reject',
    })
  })

  it('ignores final and matcher-only built-in rules', () => {
    expect(signatureFromClashRule('final')).toBeNull()
    expect(signatureFromClashRule('ip_is_private => route(direct)')).toBeNull()
  })
})

describe('isCustomRuleActive', () => {
  it('lights a structured rule when a connection reports the same matcher and outbound', () => {
    expect(isCustomRuleActive(curlDirect, [
      { rule: 'process_name=curl => route(direct)' },
    ])).toBe(true)
    expect(isCustomRuleActive(curlDirect, [
      { rule: 'process_name=wget => route(direct)' },
    ])).toBe(false)
    expect(isCustomRuleActive(curlDirect, [
      { rule: 'process_name=curl => route(proxy)' },
    ])).toBe(false)
  })

  it('does not light skipped rules', () => {
    expect(isCustomRuleActive(
      { ...curlDirect, skipped: true },
      [{ rule: 'process_name=curl => route(direct)' }],
    )).toBe(false)
  })

  it('matches handwritten JSON by the single known matcher', () => {
    const handwritten = {
      index: 2,
      raw: '{"rule_set":["custom"],"action":"route","outbound":"proxy"}',
    }
    expect(isCustomRuleActive(handwritten, [
      { rule: 'rule_set=custom => route(proxy)' },
    ])).toBe(true)
  })
})

describe('activeRuleIndexes', () => {
  it('returns indexes of rules that currently have matching connections', () => {
    const rules = [
      curlDirect,
      { index: 1, field: 'domain', value: 't.co', target: 'proxy', raw: '{}' },
    ]
    const active = activeRuleIndexes(rules, [
      { rule: 'process_name=curl => route(direct)' },
      { rule: 'final' },
    ])
    expect([...active]).toEqual([0])
  })
})

describe('signatureFromCustomRule', () => {
  it('uses structured fields when present', () => {
    expect(signatureFromCustomRule(curlDirect)).toEqual({
      field: 'process_name',
      value: 'curl',
      target: 'direct',
    })
  })
})
