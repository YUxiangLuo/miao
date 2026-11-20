import { renderHook } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { beforeEach, describe, expect, it } from '@rstest/core'
import { useTheme } from './useTheme'
import { THEME_KEY } from '../tokens'

describe('useTheme', () => {
  beforeEach(() => {
    window.localStorage.clear()
    delete document.documentElement.dataset.theme
  })

  it('默认 light（暖石白）', () => {
    const { result } = renderHook(() => useTheme())
    flush()
    expect(result.theme()).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(window.localStorage.getItem(THEME_KEY)).toBe('light')
  })

  it('light ↔ dark 切换并持久化', () => {
    const { result } = renderHook(() => useTheme())
    flush()

    result.toggle()
    flush()
    expect(result.theme()).toBe('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(window.localStorage.getItem(THEME_KEY)).toBe('dark')

    result.toggle()
    flush()
    expect(result.theme()).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(window.localStorage.getItem(THEME_KEY)).toBe('light')
  })

  it('从 localStorage 恢复显式暗色主题', () => {
    window.localStorage.setItem(THEME_KEY, 'dark')
    const { result } = renderHook(() => useTheme())
    flush()
    expect(result.theme()).toBe('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
  })

  it('历史 auto 值归一为 light', () => {
    window.localStorage.setItem(THEME_KEY, 'auto')
    const { result } = renderHook(() => useTheme())
    flush()
    expect(result.theme()).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
  })
})
