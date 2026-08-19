import { describe, expect, it } from 'vitest'
import {
  displayRuleText,
  filterConnectionGroups,
  humanizeClashRule,
  isDirectOutbound,
  pathCountsFor,
  processGroupRows,
  outboundGroupRows,
  processNameOf,
  sortConnectionGroups,
} from './connectionFilters'
import { connectionGroupMock, connectionMock } from '../testFixtures'

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

describe('processNameOf', () => {
  it('extracts the basename and strips the user suffix', () => {
    expect(processNameOf('/usr/lib/firefox/firefox (alice)')).toBe('firefox')
    expect(processNameOf('/opt/brave-origin-bin/brave (alice)')).toBe('brave')
  })

  it('handles Windows paths and NT device paths', () => {
    expect(processNameOf('C:\\Program Files\\qBittorrent\\qbittorrent.exe')).toBe('qbittorrent.exe')
    expect(processNameOf('\\Device\\HarddiskVolume3\\Windows\\System32\\svchost.exe')).toBe('svchost.exe')
  })

  it('returns null for empty input', () => {
    expect(processNameOf('')).toBeNull()
    expect(processNameOf(undefined)).toBeNull()
  })
})

describe('humanizeClashRule', () => {
  it('translates rule_set and final into friendly labels', () => {
    expect(humanizeClashRule('final')).toBe('兜底规则')
    expect(humanizeClashRule('rule_set=chinasite => route(direct)')).toBe('中国站点规则集 → 直连')
    expect(humanizeClashRule('rule_set=chinaip => route(direct)')).toBe('中国 IP 规则集 → 直连')
  })

  it('translates custom rule matchers with field labels and targets', () => {
    expect(humanizeClashRule('process_name=qbittorrent => route(direct)')).toBe('进程名 qbittorrent → 直连')
    expect(humanizeClashRule('domain_keyword=openai => route(香港节点)')).toBe('域名关键词 openai → 香港节点')
    expect(humanizeClashRule('protocol=bittorrent => reject')).toBe('嗅探协议 bittorrent → 拦截')
  })

  it('translates bare ip_is_private matcher', () => {
    expect(humanizeClashRule('ip_is_private => route(direct)')).toBe('私有 IP → 直连')
  })

  it('returns null for unparseable formats so callers fall back', () => {
    expect(humanizeClashRule('')).toBeNull()
    expect(humanizeClashRule('RuleSet')).toBeNull()
    expect(humanizeClashRule('Match')).toBeNull()
  })
})

describe('dimension group rows', () => {
  const conns = [
    connectionMock({
      id: 'a',
      downloadSpeed: 100,
      metadata: { host: 'api.github.com', processPath: '/usr/bin/brave (alice)' },
    }),
    connectionMock({
      id: 'b',
      downloadSpeed: 50,
      chains: ['direct'],
      metadata: { host: 'www.bilibili.com', processPath: '/usr/bin/brave (alice)' },
    }),
    connectionMock({
      id: 'c',
      downloadSpeed: 10,
      metadata: { host: 'mtalk.google.com', processPath: '/usr/lib/firefox/firefox (alice)' },
    }),
  ]

  it('groups by process with domain counts and summed speeds', () => {
    const rows = processGroupRows(conns)
    expect(rows).toHaveLength(2)
    const brave = rows.find((r) => r.title === 'brave')!
    expect(brave.count).toBe(2)
    expect(brave.subtitle).toBe('2 个站点')
    expect(brave.downloadSpeed).toBe(150)
    const firefox = rows.find((r) => r.title === 'firefox')!
    expect(firefox.subtitle).toBe('1 个站点')
  })

  it('groups connections without process info into 未知进程', () => {
    const rows = processGroupRows([connectionMock({ metadata: { host: 'a.dev' } })])
    expect(rows).toHaveLength(1)
    expect(rows[0].title).toBe('未知进程')
  })

  it('groups by outbound', () => {
    const rows = outboundGroupRows(conns)
    const proxy = rows.find((r) => r.title === 'proxy')!
    const direct = rows.find((r) => r.title === 'direct')!
    expect(proxy.count).toBe(2)
    expect(direct.count).toBe(1)
    expect(direct.subtitle).toBe('1 个站点')
  })
})
