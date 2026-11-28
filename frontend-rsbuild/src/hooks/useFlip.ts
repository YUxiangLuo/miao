import { createEffect, createMemo, createSignal, type Accessor } from 'solid-js'
import { peekReducedMotion } from '../motion'
import { FLIP_EASING, FLIP_MS } from '../tokens'

/**
 * 容器级 FLIP 动画（First / Last / Invert / Play）：
 * 子元素带 data-flip-key；每次 revision 变更后比对布局位置，位移的子元素
 * 用 WAAPI 从旧位置以建筑平推阻尼滑入新位置。
 */
export function useFlipContainer<T extends HTMLElement>(
  active: Accessor<boolean> = () => true,
  revision?: Accessor<string | undefined>,
) {
  const [container, setContainer] = createSignal<T | undefined>()
  const activeMemo = createMemo(() => Boolean(active()))
  const revisionMemo = createMemo(() => revision?.())
  const prevPositions = new Map<string, { top: number; left: number }>()
  let lastContainer: T | undefined
  let lastRevision: string | undefined

  createEffect(
    () => ({
      isActive: activeMemo(),
      el: container(),
      rev: revisionMemo(),
    }),
    ({ isActive, el, rev }) => {
      if (!isActive || !el) {
        prevPositions.clear()
        lastContainer = undefined
        lastRevision = undefined
        return
      }
      if (el !== lastContainer) {
        prevPositions.clear()
        lastContainer = el
        lastRevision = undefined
      }
      if (rev !== undefined && lastRevision === rev) return
      lastRevision = rev
      const reduceMotion = peekReducedMotion()
      const next = new Map<string, { top: number; left: number }>()
      for (const child of el.querySelectorAll<HTMLElement>(':scope > [data-flip-key]')) {
        const key = child.dataset.flipKey as string
        const pos = { top: child.offsetTop, left: child.offsetLeft }
        const prev = prevPositions.get(key)
        if (prev && !reduceMotion && (prev.top !== pos.top || prev.left !== pos.left)) {
          child.animate?.(
            [
              { transform: `translate(${prev.left - pos.left}px, ${prev.top - pos.top}px)` },
              { transform: 'translate(0, 0)' },
            ],
            { duration: FLIP_MS, easing: FLIP_EASING },
          )
        }
        next.set(key, pos)
      }
      prevPositions.clear()
      next.forEach((value, key) => prevPositions.set(key, value))
    },
  )

  return (el: T) => {
    setContainer(() => el)
  }
}
