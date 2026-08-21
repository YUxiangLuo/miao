import {
  HOME_CONNECTIONS_MORE_WIDTH,
  HOME_SITE_CARD_GAP,
  HOME_SITE_CARD_WIDTH,
} from '../tokens'

export interface HomeConnectionFoldPlan {
  shown: number
  more: number
}

/** 按条带实测宽度折叠站点卡；溢出时为紧凑的 +N 入口预留空间。 */
export function foldHomeConnections(total: number, width: number): HomeConnectionFoldPlan {
  if (total <= 0 || !Number.isFinite(width)) return { shown: Math.max(0, total), more: 0 }

  const allCardsWidth = total * HOME_SITE_CARD_WIDTH + Math.max(0, total - 1) * HOME_SITE_CARD_GAP
  if (allCardsWidth <= width) return { shown: total, more: 0 }

  const availableForCards = Math.max(0, width - HOME_CONNECTIONS_MORE_WIDTH - HOME_SITE_CARD_GAP)
  const shown = Math.min(
    total,
    Math.max(0, Math.floor((availableForCards + HOME_SITE_CARD_GAP) / (HOME_SITE_CARD_WIDTH + HOME_SITE_CARD_GAP))),
  )
  return { shown, more: total - shown }
}
