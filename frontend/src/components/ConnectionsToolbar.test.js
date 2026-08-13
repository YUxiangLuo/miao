import { describe, expect, it } from 'vitest'
import {
  filterConnectionGroups,
  isDirectOutbound,
  pathCountsFor,
  sortConnectionGroups,
} from './connectionFilters.js'

const groups = [
  { domain: 'api.github.com', outbound: 'vps-1', rule: 'final', count: 2, downloadSpeed: 800 },
  { domain: 'www.bilibili.com', outbound: 'direct', rule: 'rule_set=chinasite => route(direct)', count: 5, downloadSpeed: 120 },
  { domain: 'chatgpt.com', outbound: 'vps-1', rule: 'final', count: 1, downloadSpeed: 40 },
]

describe('connection filters', () => {
  it('classifies direct vs proxy outbounds', () => {
    expect(isDirectOutbound('direct')).toBe(true)
    expect(isDirectOutbound('Direct')).toBe(true)
    expect(isDirectOutbound('vps-1')).toBe(false)
  })

  it('counts path chips from the current search set', () => {
    expect(pathCountsFor(groups)).toEqual({ all: 3, proxy: 2, direct: 1 })
  })

  it('filters by query across domain, rule, and outbound', () => {
    expect(filterConnectionGroups(groups, { query: 'bili' }).map((group) => group.domain))
      .toEqual(['www.bilibili.com'])
    expect(filterConnectionGroups(groups, { query: 'final' }).map((group) => group.domain))
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
  it('sorts by activity, domain, or connection count', () => {
    expect(sortConnectionGroups(groups, 'activity').map((group) => group.domain))
      .toEqual(['api.github.com', 'www.bilibili.com', 'chatgpt.com'])
    expect(sortConnectionGroups(groups, 'domain').map((group) => group.domain))
      .toEqual(['api.github.com', 'chatgpt.com', 'www.bilibili.com'])
    expect(sortConnectionGroups(groups, 'count').map((group) => group.domain))
      .toEqual(['www.bilibili.com', 'api.github.com', 'chatgpt.com'])
  })
})
