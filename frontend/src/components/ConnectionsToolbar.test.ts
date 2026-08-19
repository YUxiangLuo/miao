import { describe, expect, it } from 'vitest'
import {
  displayRuleText,
  filterConnectionGroups,
  isDirectOutbound,
  pathCountsFor,
  sortConnectionGroups,
} from './connectionFilters'
import { connectionGroupMock } from '../testFixtures'

const groups = [
  connectionGroupMock({
    domain: 'api.github.com',
    outbound: 'vps-1',
    rule: 'final',
    ruleLabel: '兜底规则',
    count: 2,
    downloadSpeed: 800,
    uploadSpeed: 0,
    download: 5000,
    upload: 100,
  }),
  connectionGroupMock({
    domain: 'www.bilibili.com',
    outbound: 'direct',
    rule: 'rule_set=chinasite => route(direct)',
    ruleLabel: 'rule_set=chinasite => route(direct)',
    count: 5,
    downloadSpeed: 120,
    uploadSpeed: 30,
    download: 9000,
    upload: 500,
  }),
  connectionGroupMock({
    domain: 'chatgpt.com',
    outbound: 'vps-1',
    rule: 'final',
    ruleLabel: '兜底规则',
    count: 1,
    downloadSpeed: 40,
    uploadSpeed: 10,
    download: 100,
    upload: 50,
  }),
]

describe('connection filters', () => {
  it('classifies direct vs proxy outbounds', () => {
    expect(isDirectOutbound('direct')).toBe(true)
    expect(isDirectOutbound('Direct')).toBe(true)
    expect(isDirectOutbound('vps-1')).toBe(false)
  })

  it('translates the final rule into a friendly label', () => {
    expect(displayRuleText('final')).toBe('兜底规则')
    expect(displayRuleText('RuleSet : chinasite')).toBe('RuleSet : chinasite')
    expect(displayRuleText('')).toBe('-')
    expect(displayRuleText('-')).toBe('-')
  })

  it('counts path chips from the current search set', () => {
    expect(pathCountsFor(groups)).toEqual({ all: 3, proxy: 2, direct: 1 })
  })

  it('filters by query across domain, rule, and outbound', () => {
    expect(filterConnectionGroups(groups, { query: 'bili' }).map((group) => group.domain))
      .toEqual(['www.bilibili.com'])
    expect(filterConnectionGroups(groups, { query: 'final' }).map((group) => group.domain))
      .toEqual(['api.github.com', 'chatgpt.com'])
    expect(filterConnectionGroups(groups, { query: '兜底' }).map((group) => group.domain))
      .toEqual(['api.github.com', 'chatgpt.com'])
    expect(filterConnectionGroups(groups, { query: 'vps-1' })).toHaveLength(2)
  })

  it('filters by proxy or direct path', () => {
    expect(filterConnectionGroups(groups, { path: 'direct' }).map((group) => group.domain))
      .toEqual(['www.bilibili.com'])
    expect(filterConnectionGroups(groups, { path: 'proxy' }).map((group) => group.domain))
      .toEqual(['api.github.com', 'chatgpt.com'])
  })
})

describe('connection sorting', () => {
  it('sorts by combined up/down speed (fixed order)', () => {
    expect(sortConnectionGroups(groups).map((group) => group.domain))
      .toEqual(['api.github.com', 'www.bilibili.com', 'chatgpt.com'])
  })
})
