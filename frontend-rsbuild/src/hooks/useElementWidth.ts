import { useLayoutEffect, useRef, useState } from 'react'

/**
 * 实测元素宽度（ResizeObserver，useLayoutEffect 首帧前就绪，无闪烁）。
 * 无 ResizeObserver 时降级监听 window resize；无布局引擎（如 jsdom）返回
 * clientWidth=0 时保持 Infinity，由调用方按「不裁剪」处理。
 */
export function useElementWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  const [width, setWidth] = useState(Infinity)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const update = () => setWidth(el.clientWidth > 0 ? el.clientWidth : Infinity)
    update()

    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', update)
      return () => window.removeEventListener('resize', update)
    }

    const observer = new ResizeObserver(update)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return { ref, width }
}
