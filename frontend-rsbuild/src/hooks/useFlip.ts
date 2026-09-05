import { useLayoutEffect, useRef } from 'react'
import type { RefObject } from 'react'
import { FLIP_MS } from '../tokens'

/**
 * 容器级 FLIP 动画（First / Last / Invert / Play）：
 * 子元素带 data-flip-key；每次渲染后比对布局位置，位移的子元素
 * 用 WAAPI 从旧位置滑入新位置。只动画「移动」，新进/消失不做插入/退出动画。
 *
 * 前提：容器 position: relative——子元素 offsetTop 相对它测量，
 * 列表自身滚动不会改变该值，滚动不会误触动画。
 * 遵守 prefers-reduced-motion；jsdom 无 WAAPI 时静默跳过（el.animate?.）。
 *
 * active=false（如弹窗关闭）时清空位置记录：下次激活是全新布局，
 * 不应从陈旧位置滑入。
 */
export function useFlipContainer<T extends HTMLElement>(active = true, revision?: string): RefObject<T | null> {
  const containerRef = useRef<T>(null)
  // 上一帧布局位置（offsetTop/offsetLeft，相对容器，滚动安全）
  const prevPositionsRef = useRef(new Map<string, { top: number; left: number }>())

  const lastRevision = useRef<string | undefined>(undefined)
  useLayoutEffect(() => {
    if (!active) {
      prevPositionsRef.current.clear()
      lastRevision.current = undefined
      return
    }
    const container = containerRef.current
    if (!container) {
      // 行容器未挂载（筛选无匹配 / 数据面未就绪的空态）：与失活同理，从空白开始
      prevPositionsRef.current.clear()
      lastRevision.current = undefined
      return
    }
    if (revision !== undefined && lastRevision.current === revision) return
    lastRevision.current = revision
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const next = new Map<string, { top: number; left: number }>()
    for (const el of container.querySelectorAll<HTMLElement>(':scope > [data-flip-key]')) {
      const key = el.dataset.flipKey as string
      const pos = { top: el.offsetTop, left: el.offsetLeft }
      const prev = prevPositionsRef.current.get(key)
      if (prev && !reduceMotion && (prev.top !== pos.top || prev.left !== pos.left)) {
        // Invert + Play：先视觉放回旧位置，再滑到新位置
        el.animate?.(
          [
            { transform: `translate(${prev.left - pos.left}px, ${prev.top - pos.top}px)` },
            { transform: 'translate(0, 0)' },
          ],
          { duration: FLIP_MS, easing: 'ease' },
        )
      }
      next.set(key, pos)
    }
    prevPositionsRef.current = next
  })

  return containerRef
}
