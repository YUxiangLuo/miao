import { useCallback, useEffect, useState } from 'react'
import { THEME_KEY, THEME_META } from '../tokens.js'

function apply(theme) {
  document.documentElement.dataset.theme = theme
  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta) meta.content = THEME_META[theme] || THEME_META.dark
}

// 只有显式 dark/light 两态；任何历史值（如旧版的 auto）都归一为 dark 默认
function readStored() {
  try {
    return window.localStorage.getItem(THEME_KEY) === 'light' ? 'light' : 'dark'
  } catch {
    return 'dark'
  }
}

// 初始值已由 index.html 引导脚本落到 documentElement 上，
// 这里负责后续切换与持久化。
export function useTheme() {
  const [theme, setTheme] = useState(readStored)

  useEffect(() => {
    apply(theme)
    try {
      window.localStorage.setItem(THEME_KEY, theme)
    } catch {
      /* 隐私模式等不可写场景静默跳过 */
    }
  }, [theme])

  const toggle = useCallback(() => {
    setTheme((t) => (t === 'dark' ? 'light' : 'dark'))
  }, [])

  return { theme, toggle }
}
