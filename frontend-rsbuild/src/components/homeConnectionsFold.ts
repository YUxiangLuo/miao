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

/**
 * Maintains stable display order for active site cards across 3s connection polls:
 * - Newly active group IDs (already sorted by speed descending) are prepended to the front.
 * - Continuing groups keep their previous relative positions instead of jumping on speed fluctuations.
 * - Groups that dropped to 0 speed or vanished are evicted so a later reappearance enters at the front.
 */
export function mergeActiveGroupOrder<T extends { id: string }>(
  previousOrder: readonly string[],
  activeGroups: readonly T[],
): { nextOrder: string[]; orderedGroups: T[] } {
  const byId = new Map(activeGroups.map((group) => [group.id, group]))
  const prevSet = new Set(previousOrder)
  const freshIds = activeGroups.map((group) => group.id).filter((id) => !prevSet.has(id))
  const keptIds = previousOrder.filter((id) => byId.has(id))
  const nextOrder = [...freshIds, ...keptIds]
  return {
    nextOrder,
    orderedGroups: nextOrder.map((id) => byId.get(id)!),
  }
}
