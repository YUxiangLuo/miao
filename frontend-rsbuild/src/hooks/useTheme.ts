import { createEffect, createSignal } from 'solid-js'
import { THEME_KEY, THEME_META } from '../tokens'

// 显式 light / dark 两态，默认 light（ATELIER GRID 暖石白），不跟随系统
export type Theme = 'light' | 'dark'

function apply(theme: Theme): void {
  document.documentElement.dataset.theme = theme
  const meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')
  if (meta) {
    const cssBg = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()
    meta.content = cssBg || THEME_META[theme] || THEME_META.light
  }
}

function readStored(): Theme {
  try {
    return window.localStorage.getItem(THEME_KEY) === 'dark' ? 'dark' : 'light'
  } catch {
    return 'light'
  }
}

export function useTheme() {
  const [theme, setTheme] = createSignal<Theme>(readStored())

  createEffect(
    () => theme(),
    (current) => {
      apply(current)
      try {
        window.localStorage.setItem(THEME_KEY, current)
      } catch {
        /* 隐私模式等不可写场景静默跳过 */
      }
    },
  )

  const toggle = () => {
    setTheme((t) => (t === 'light' ? 'dark' : 'light'))
  }

  return { theme, toggle }
}
