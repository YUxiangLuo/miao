import { useCallback, useEffect, useState } from 'react'
import { THEME_KEY, THEME_META } from '../tokens.js'

const LIGHT_MQ = '(prefers-color-scheme: light)'

// 'auto' 时跟随系统；显式 light/dark 直接生效
function resolve(theme) {
  if (theme !== 'auto') return theme
  if (typeof window.matchMedia !== 'function') return 'dark'
  return window.matchMedia(LIGHT_MQ).matches ? 'light' : 'dark'
}

function apply(theme) {
  const resolved = resolve(theme)
  document.documentElement.dataset.theme = resolved
  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta) meta.content = THEME_META[resolved] || THEME_META.dark
}

function readStored() {
  try {
    return window.localStorage.getItem(THEME_KEY) || 'auto'
  } catch {
    return 'auto'
  }
}

// 主题三态循环：auto → light → dark。初始值已由 index.html 引导脚本落到
// documentElement 上，这里负责后续切换、持久化与跟随系统变化。
export function useTheme() {
  const [theme, setTheme] = useState(readStored)

  useEffect(() => {
    apply(theme)
    try {
      window.localStorage.setItem(THEME_KEY, theme)
    } catch {
      /* 隐私模式等不可写场景静默跳过 */
    }
    if (theme !== 'auto') return undefined
    const mq = window.matchMedia(LIGHT_MQ)
    const onChange = () => apply('auto')
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [theme])

  const cycle = useCallback(() => {
    setTheme((t) => (t === 'auto' ? 'light' : t === 'light' ? 'dark' : 'auto'))
  }, [])

  return { theme, cycle }
}
