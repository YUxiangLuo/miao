import type { RuleInfo } from '../types/api'
import type { EnrichedConnection } from '../types/clash'
import { connectionDomain, normalizeDomain } from '../components/siteIcons'

export type TradeCategoryId = 'social' | 'ecommerce' | 'payment' | 'productivity' | 'domestic'

export interface TradeCategory {
  id: TradeCategoryId
  label: string
  description: string
}

export interface TradePlatform {
  id: string
  name: string
  shortLabel: string
  category: TradeCategoryId
  accent: string
  description: string
  /** 写入自定义分流规则时推荐的主域名后缀（第一项作为一键规则默认后缀） */
  primarySuffix: string
  /** 用于实时连接识别的全部关联域名后缀 */
  matchSuffixes: string[]
  recommendedTarget: 'proxy' | 'dedicated' | 'direct'
}

export const TRADE_CATEGORIES: TradeCategory[] = [
  {
    id: 'social',
    label: '海外社媒与客户沟通',
    description: 'WhatsApp 客户跟单、LinkedIn 开发信、TikTok 短视频与独立站引流',
  },
  {
    id: 'ecommerce',
    label: '跨境电商卖家后台',
    description: 'Amazon Seller / Shopify / eBay 等多店铺运营，强烈建议配合多店铺专属端口锁死静态住宅 IP',
  },
  {
    id: 'payment',
    label: '跨境收款与外币结算',
    description: 'PayPal / Stripe / Wise / Payoneer 风控敏感业务，需固定出口防止异地触发二审',
  },
  {
    id: 'productivity',
    label: '海外邮件、检索与 AI 办公',
    description: 'Gmail 企业邮、Google 海关背调、ChatGPT / Claude 多语种外贸文案翻译',
  },
  {
    id: 'domestic',
    label: '国内办公与网银直连保护',
    description: '企业微信、飞书、钉钉、ERP 及各大银行对公网银，内置大陆规则集保障毫秒级本地直连',
  },
]

export const TRADE_PLATFORMS: TradePlatform[] = [
  {
    id: 'whatsapp',
    name: 'WhatsApp Business',
    shortLabel: 'WA',
    category: 'social',
    accent: '#16a34a',
    description: '外贸客户即时沟通、语音通话与文件收发',
    primarySuffix: 'whatsapp.net',
    matchSuffixes: ['whatsapp.com', 'whatsapp.net'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'linkedin',
    name: 'LinkedIn 领英',
    shortLabel: 'IN',
    category: 'social',
    accent: '#0284c7',
    description: 'B2B 采购商背调与精准客户开发',
    primarySuffix: 'linkedin.com',
    matchSuffixes: ['linkedin.com', 'licdn.com'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'tiktok',
    name: 'TikTok 海外运营',
    shortLabel: 'TK',
    category: 'social',
    accent: '#0f172a',
    description: 'TikTok Shop 直播、短视频矩阵与达人建联',
    primarySuffix: 'tiktok.com',
    matchSuffixes: ['tiktok.com', 'tiktokv.com', 'tiktokcdn.com', 'byteoversea.com'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'meta',
    name: 'Meta / Instagram',
    shortLabel: 'FB',
    category: 'social',
    accent: '#2563eb',
    description: 'Facebook 广告投放、主页询盘与 Instagram 社媒',
    primarySuffix: 'facebook.com',
    matchSuffixes: ['facebook.com', 'fbcdn.net', 'instagram.com', 'cdninstagram.com', 'messenger.com'],
    recommendedTarget: 'proxy',
  },
  {
    id: 'telegram',
    name: 'Telegram',
    shortLabel: 'TG',
    category: 'social',
    accent: '#0ea5e9',
    description: '东欧/中东/南美批发客户即时联络',
    primarySuffix: 'telegram.org',
    matchSuffixes: ['telegram.org', 't.me', 'telesco.pe'],
    recommendedTarget: 'proxy',
  },
  {
    id: 'amazon',
    name: 'Amazon 卖家中心',
    shortLabel: 'AMZ',
    category: 'ecommerce',
    accent: '#d97706',
    description: '多站点卖家后台（多店同机请使用「多店铺专属端口」）',
    primarySuffix: 'amazon.com',
    matchSuffixes: ['amazon.com', 'amazon.co.uk', 'amazon.de', 'amazon.co.jp', 'sellercentral.amazon.com', 'media-amazon.com'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'shopify',
    name: 'Shopify 独立站',
    shortLabel: 'SHP',
    category: 'ecommerce',
    accent: '#059669',
    description: '品牌独立站装修、订单履约与插件管理',
    primarySuffix: 'shopify.com',
    matchSuffixes: ['shopify.com', 'myshopify.com', 'shopifycdn.com'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'aliexpress',
    name: '阿里国际站 / 速卖通',
    shortLabel: 'ALI',
    category: 'ecommerce',
    accent: '#ea580c',
    description: 'B2B 询盘回复、RFQ 报价与跨境零售后台',
    primarySuffix: 'aliexpress.com',
    matchSuffixes: ['aliexpress.com', 'alibaba.com'],
    recommendedTarget: 'proxy',
  },
  {
    id: 'paypal',
    name: 'PayPal 企业收款',
    shortLabel: 'PP',
    category: 'payment',
    accent: '#1d4ed8',
    description: '样品单收款与争议处理，严禁频繁切换国家 IP',
    primarySuffix: 'paypal.com',
    matchSuffixes: ['paypal.com', 'paypalobjects.com'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'stripe',
    name: 'Stripe / Wise 结算',
    shortLabel: 'STR',
    category: 'payment',
    accent: '#4f46e5',
    description: '独立站信用卡收单与多币种离岸账户管理',
    primarySuffix: 'stripe.com',
    matchSuffixes: ['stripe.com', 'wise.com', 'payoneer.com'],
    recommendedTarget: 'dedicated',
  },
  {
    id: 'google',
    name: 'Google / Gmail 企业邮',
    shortLabel: 'G',
    category: 'productivity',
    accent: '#2563eb',
    description: '海外客户邮件往来、Google 搜索背调与云端表格',
    primarySuffix: 'google.com',
    matchSuffixes: ['google.com', 'gmail.com', 'googleapis.com', 'gstatic.com'],
    recommendedTarget: 'proxy',
  },
  {
    id: 'ai',
    name: 'ChatGPT / Claude AI',
    shortLabel: 'AI',
    category: 'productivity',
    accent: '#0d9488',
    description: '开发信润色、小语种客服翻译与选品市场调研',
    primarySuffix: 'openai.com',
    matchSuffixes: ['openai.com', 'chatgpt.com', 'claude.ai', 'anthropic.com', 'gemini.google.com'],
    recommendedTarget: 'proxy',
  },
  {
    id: 'wecom',
    name: '企业微信 / 微信',
    shortLabel: '企微',
    category: 'domestic',
    accent: '#059669',
    description: '内部协同、货代沟通与工厂对接，全程本地直连不绕海外',
    primarySuffix: 'qq.com',
    matchSuffixes: ['weixin.qq.com', 'work.weixin.qq.com', 'qq.com', 'wechat.com'],
    recommendedTarget: 'direct',
  },
  {
    id: 'feishu',
    name: '飞书 / 钉钉 / ERP',
    shortLabel: '办公',
    category: 'domestic',
    accent: '#0284c7',
    description: '团队文档、OA 审批、店小秘/领星 ERP 与对公网银直连保护',
    primarySuffix: 'feishu.cn',
    matchSuffixes: ['feishu.cn', 'dingtalk.com', '1688.com', 'dianxiaomi.com', 'lingxing.com'],
    recommendedTarget: 'direct',
  },
]

const SUFFIX_LOOKUP = TRADE_PLATFORMS
  .flatMap((platform) => platform.matchSuffixes.map((suffix) => ({ suffix, platform })))
  .sort((a, b) => b.suffix.length - a.suffix.length)

export function matchTradePlatform(domain: string): TradePlatform | null {
  const normalized = normalizeDomain(domain)
  if (!normalized || normalized === 'unknown') return null
  const hit = SUFFIX_LOOKUP.find(({ suffix }) => (
    normalized === suffix || normalized.endsWith(`.${suffix}`)
  ))
  return hit ? hit.platform : null
}

export interface ActivePlatformSummary {
  platform: TradePlatform
  connectionCount: number
  downloadSpeed: number
  uploadSpeed: number
  outbounds: string[]
}

export function summarizeActiveTradePlatforms(connections: EnrichedConnection[]): ActivePlatformSummary[] {
  const map = new Map<string, {
    platform: TradePlatform
    connectionCount: number
    downloadSpeed: number
    uploadSpeed: number
    outbounds: Set<string>
  }>()

  for (const conn of connections) {
    const domain = connectionDomain(conn)
    const platform = matchTradePlatform(domain)
    if (!platform) continue
    const outbound = conn.chains?.[0] || conn.rule || 'direct'
    const existing = map.get(platform.id)
    if (existing) {
      existing.connectionCount += 1
      existing.downloadSpeed += Number(conn.downloadSpeed || 0)
      existing.uploadSpeed += Number(conn.uploadSpeed || 0)
      existing.outbounds.add(outbound)
    } else {
      map.set(platform.id, {
        platform,
        connectionCount: 1,
        downloadSpeed: Number(conn.downloadSpeed || 0),
        uploadSpeed: Number(conn.uploadSpeed || 0),
        outbounds: new Set([outbound]),
      })
    }
  }

  return [...map.values()]
    .map((entry) => ({
      platform: entry.platform,
      connectionCount: entry.connectionCount,
      downloadSpeed: entry.downloadSpeed,
      uploadSpeed: entry.uploadSpeed,
      outbounds: [...entry.outbounds],
    }))
    .sort((a, b) => (
      (b.downloadSpeed + b.uploadSpeed) - (a.downloadSpeed + a.uploadSpeed)
      || b.connectionCount - a.connectionCount
    ))
}

export function findPlatformPolicyRule(platform: TradePlatform, rules: RuleInfo[]): RuleInfo | undefined {
  return rules.find((rule) => (
    rule.field === 'domain_suffix'
    && rule.value != null
    && platform.matchSuffixes.includes(rule.value.trim().toLowerCase())
  ))
}
