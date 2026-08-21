import { describe, expect, it } from '@rstest/core'
import {
  displayRuleText,
  filterConnectionsByPath,
  humanizeClashRule,
  isDirectOutbound,
  pathCountsForConnections,
  processNameOf,
  sortConnectionGroups,
  sortConnections,
  uniqueIconDomains,
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

describe('uniqueIconDomains', () => {
  it('dedupes by brand id and by fallback letter', () => {
    expect(uniqueIconDomains(['x.com', 'api.x.com', 'github.com', 'api.github.com']))
      .toEqual(['x.com', 'github.com'])
    // 不同字母的未收录域名不折叠，同字母折叠
    expect(uniqueIconDomains(['alpha.dev', 'beta.dev', 'blob.dev'])).toEqual(['alpha.dev', 'beta.dev'])
  })
})

describe('per-connection path filters', () => {
  const conns = [
    connectionMock({ id: 'a', chains: ['proxy'], downloadSpeed: 100, metadata: { host: 'api.github.com' } }),
    connectionMock({ id: 'b', chains: ['direct'], downloadSpeed: 50, metadata: { host: 'www.bilibili.com' } }),
    connectionMock({ id: 'c', chains: ['香港节点'], downloadSpeed: 10, metadata: { host: 'x.com' } }),
  ]

  it('counts connections by path', () => {
    expect(pathCountsForConnections(conns)).toEqual({ all: 3, proxy: 2, direct: 1 })
  })

  it('filters by direct or proxy path', () => {
    expect(filterConnectionsByPath(conns, 'direct').map((c) => c.id)).toEqual(['b'])
    expect(filterConnectionsByPath(conns, 'proxy').map((c) => c.id)).toEqual(['a', 'c'])
    expect(filterConnectionsByPath(conns, 'all')).toHaveLength(3)
  })

  it('sorts connections by combined speed then domain', () => {
    expect(sortConnections(conns).map((c) => c.id)).toEqual(['a', 'b', 'c'])
    const idle = [
      connectionMock({ id: 'z', metadata: { host: 'z.dev' } }),
      connectionMock({ id: 'y', metadata: { host: 'a.dev' } }),
    ]
    expect(sortConnections(idle).map((c) => c.id)).toEqual(['y', 'z'])
  })
})
