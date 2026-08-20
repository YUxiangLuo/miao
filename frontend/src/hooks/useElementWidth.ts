import { useLayoutEffect, useRef, useState } from 'react'

/**
 * 实测元素宽度（ResizeObserver，useLayoutEffect 首帧前就绪，无闪烁）。
 * jsdom 等无 ResizeObserver 的环境恒为 Infinity（调用方按「不裁剪」处理）。
 */
export function useElementWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  const [width, setWidth] = useState(Infinity)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const update = () => setWidth(el.clientWidth)
    update()
    const observer = new ResizeObserver(update)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return { ref, width }
}
