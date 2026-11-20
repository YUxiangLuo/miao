import { describe, expect, it } from '@rstest/core'
import { buildProxyRegionBuckets, classifyProxyRegion, filterAndPinProxyNodes } from './proxyRegions'

describe('proxyRegions', () => {
  it('classifies Chinese, English, and flag-prefixed node names accurately', () => {
    expect(classifyProxyRegion('🇭🇰 香港宽频B0')).toBe('hk')
    expect(classifyProxyRegion('TW-Edge-01')).toBe('tw')
    expect(classifyProxyRegion('🇯🇵 东京原生 · 03')).toBe('jp')
    expect(classifyProxyRegion('Singapore AnyTLS')).toBe('sg')
    expect(classifyProxyRegion('🇺🇸 美国无痕B3')).toBe('us')
    expect(classifyProxyRegion('伦敦金融 · LON')).toBe('eu')
    expect(classifyProxyRegion('🇯🇵 东京 · 03 · Hy2 · 中转 B · 香港入口')).toBe('jp')
    expect(classifyProxyRegion('vps-34-97-90-168')).toBe('other')
  })

  it('builds non-empty region buckets in canonical order', () => {
    const nodes = ['vps-1', '🇭🇰 香港B0', '🇭🇰 香港B1', '🇺🇸 美国D4', '🇯🇵 东京01']
    expect(buildProxyRegionBuckets(nodes)).toEqual([
      { id: 'all', label: '全部', count: 5 },
      { id: 'hk', label: '香港', count: 2 },
      { id: 'jp', label: '日本', count: 1 },
      { id: 'us', label: '美国', count: 1 },
      { id: 'other', label: '其他', count: 1 },
    ])
  })

  it('pins the active node to the front without duplicating it', () => {
    const nodes = ['vps-1', '🇭🇰 香港B0', '🇺🇸 美国无痕B3', '🇯🇵 东京01']
    expect(filterAndPinProxyNodes(nodes, '🇺🇸 美国无痕B3', 'all', '')).toEqual([
      '🇺🇸 美国无痕B3',
      'vps-1',
      '🇭🇰 香港B0',
      '🇯🇵 东京01',
    ])
  })

  it('does not leak an out-of-region active node into another region filter', () => {
    const nodes = ['vps-1', '🇭🇰 香港B0', '🇭🇰 香港B1', '🇺🇸 美国无痕B3']
    expect(filterAndPinProxyNodes(nodes, '🇺🇸 美国无痕B3', 'hk', '')).toEqual([
      '🇭🇰 香港B0',
      '🇭🇰 香港B1',
    ])
  })

  it('combines region and case-insensitive search filtering', () => {
    const nodes = ['🇺🇸 美国宽频A1', '🇺🇸 美国无痕B3', '🇭🇰 香港无痕B4']
    expect(filterAndPinProxyNodes(nodes, '🇺🇸 美国无痕B3', 'us', '无痕')).toEqual(['🇺🇸 美国无痕B3'])
    expect(filterAndPinProxyNodes(nodes, '🇺🇸 美国无痕B3', 'all', 'b4')).toEqual(['🇭🇰 香港无痕B4'])
  })
})
