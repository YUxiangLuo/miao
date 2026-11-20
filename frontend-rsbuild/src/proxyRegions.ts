export type ProxyRegionId = 'all' | 'hk' | 'tw' | 'jp' | 'sg' | 'us' | 'eu' | 'other'

export interface ProxyRegionBucket {
  id: ProxyRegionId
  label: string
  count: number
}

const REGION_RULES: ReadonlyArray<{ id: Exclude<ProxyRegionId, 'all' | 'other'>; label: string; pattern: RegExp }> = [
  { id: 'hk', label: '香港', pattern: /🇭🇰|香港|(?<![a-zA-Z])(?:HK|HKG)(?![a-zA-Z])|Hong\s*Kong/i },
  { id: 'tw', label: '台湾', pattern: /🇹🇼|台湾|臺灣|台北|(?<![a-zA-Z])(?:TW|TPE)(?![a-zA-Z])|Taiwan/i },
  { id: 'jp', label: '日本', pattern: /🇯🇵|日本|东京|東京|大阪|(?<![a-zA-Z])(?:JP|JPN|TYO|OSA)(?![a-zA-Z])|Japan|Tokyo|Osaka/i },
  { id: 'sg', label: '新加坡', pattern: /🇸🇬|新加坡|狮城|獅城|(?<![a-zA-Z])(?:SG|SGP|SIN)(?![a-zA-Z])|Singapore/i },
  { id: 'us', label: '美国', pattern: /🇺🇸|美国|美國|洛杉矶|硅谷|圣何塞|纽约|西雅图|芝加哥|达拉斯|(?<![a-zA-Z])(?:US|USA|LAX|SJC|NYC|SEA)(?![a-zA-Z])|United\s*States|America/i },
  { id: 'eu', label: '欧洲', pattern: /🇬🇧|🇩🇪|🇫🇷|🇳🇱|🇨🇭|🇮🇪|🇸🇪|🇫🇮|🇮🇹|🇪🇸|🇵🇱|🇦🇹|🇧🇪|🇳🇴|🇩🇰|英国|英國|德国|德國|法国|法國|荷兰|荷蘭|瑞士|爱尔兰|瑞典|芬兰|意大利|西班牙|波兰|欧洲|歐洲|伦敦|法兰克福|阿姆斯特丹|巴黎|(?<![a-zA-Z])(?:UK|GB|DE|FR|NL|CH|IE|SE|FI|IT|ES|EU|LON|FRA|AMS)(?![a-zA-Z])/i },
]

/**
 * 根据节点名称识别所属出口地区；未命中归入 other（如自建 VPS、韩国、澳洲等）。
 * 当名称同时包含多个地名（如「🇯🇵 东京 · 中转 · 香港入口」）时，优先取最靠前出现的地区标识，
 * 避免中转入口地名覆盖真实落地地区。
 */
export function classifyProxyRegion(nodeName: string): Exclude<ProxyRegionId, 'all'> {
  let bestRegion: Exclude<ProxyRegionId, 'all'> = 'other'
  let bestIndex = Infinity
  for (const rule of REGION_RULES) {
    const match = rule.pattern.exec(nodeName)
    if (match && match.index < bestIndex) {
      bestIndex = match.index
      bestRegion = rule.id
    }
  }
  return bestRegion
}

/** 统计各地区节点数量，仅返回非空地区分组（始终以「全部」开头）。 */
export function buildProxyRegionBuckets(nodeNames: readonly string[]): ProxyRegionBucket[] {
  const counts = new Map<Exclude<ProxyRegionId, 'all'>, number>()
  for (const name of nodeNames) {
    const region = classifyProxyRegion(name)
    counts.set(region, (counts.get(region) ?? 0) + 1)
  }

  const buckets: ProxyRegionBucket[] = [{ id: 'all', label: '全部', count: nodeNames.length }]
  for (const rule of REGION_RULES) {
    const count = counts.get(rule.id) ?? 0
    if (count > 0) buckets.push({ id: rule.id, label: rule.label, count })
  }
  const otherCount = counts.get('other') ?? 0
  if (otherCount > 0) buckets.push({ id: 'other', label: '其他', count: otherCount })
  return buckets
}

/**
 * 过滤并排序待展示的节点：
 * - 按地区与关键词过滤
 * - 若当前选中节点（activeNode）在过滤结果中，始终置顶到首位，避免百级节点池中找不到当前出口
 */
export function filterAndPinProxyNodes(
  nodeNames: readonly string[],
  activeNode: string | undefined,
  region: ProxyRegionId,
  query: string,
): string[] {
  const needle = query.trim().toLowerCase()
  const filtered = nodeNames.filter((name) => {
    if (region !== 'all' && classifyProxyRegion(name) !== region) return false
    if (needle && !name.toLowerCase().includes(needle)) return false
    return true
  })

  if (!activeNode) return filtered
  const activeIndex = filtered.indexOf(activeNode)
  if (activeIndex <= 0) return filtered
  return [activeNode, ...filtered.slice(0, activeIndex), ...filtered.slice(activeIndex + 1)]
}
