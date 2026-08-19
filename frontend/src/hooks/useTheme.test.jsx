import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import { useTheme } from './useTheme.js'
import { THEME_KEY } from '../tokens.js'

describe('useTheme', () => {
  beforeEach(() => {
    window.localStorage.clear()
    delete document.documentElement.dataset.theme
  })

  it('默认 auto，并按系统偏好吧 data-theme 落到具体主题', () => {
    const { result } = renderHook(() => useTheme())
    expect(result.current.theme).toBe('auto')
    // jsdom 环境 matchMedia 被 stub 为 matches: false → 解析为 dark
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(window.localStorage.getItem(THEME_KEY)).toBe('auto')
  })

  it('按 auto → light → dark → auto 循环并持久化', () => {
    const { result } = renderHook(() => useTheme())

    act(() => result.current.cycle())
    expect(result.current.theme).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(window.localStorage.getItem(THEME_KEY)).toBe('light')

    act(() => result.current.cycle())
    expect(result.current.theme).toBe('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')

    act(() => result.current.cycle())
    expect(result.current.theme).toBe('auto')
    expect(window.localStorage.getItem(THEME_KEY)).toBe('auto')
  })

  it('从 localStorage 恢复显式主题', () => {
    window.localStorage.setItem(THEME_KEY, 'light')
    const { result } = renderHook(() => useTheme())
    expect(result.current.theme).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
  })
})
